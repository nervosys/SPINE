//! Pluggable chat model backend for the gRPC `StreamChat` RPC.
//!
//! `StreamChat` no longer hard-codes a demo generator: it delegates to a
//! [`ChatModel`], so a deployment plugs in a real model. Two implementations
//! ship here:
//!
//! - [`OpenAiChatModel`] — backs `StreamChat` with **any OpenAI-compatible
//!   `/v1/chat/completions` endpoint** (OpenAI, Together, Groq, vLLM, or
//!   SPINE's own gateway), streaming `chat.completion.chunk` SSE deltas. Lazy:
//!   it pulls tokens from the upstream as the gRPC consumer pulls them, so when
//!   the client cancels the gRPC stream, upstream generation stops too.
//! - [`EchoModel`] — a hermetic reference/test model that streams the prompt
//!   back word by word. No network.
//!
//! The SSE wire parsing lives in [`SseDecoder`] (a pure, unit-tested state
//! machine) and the byte-stream→delta logic in [`deltas_from_byte_stream`], so
//! the real decoding path is tested without a network round-trip.

use std::pin::Pin;

use bytes::Bytes;
use futures_core::Stream;
use futures_util::StreamExt;
use serde::Deserialize;

/// A request to generate a completion.
#[derive(Debug, Clone)]
pub struct ChatRequest {
    /// Model name to ask the backend for.
    pub model: String,
    /// The prompt / user turn.
    pub prompt: String,
}

/// One streamed increment of a completion.
#[derive(Debug, Clone, PartialEq)]
pub struct ChatDelta {
    /// Text fragment for this step (empty on the terminal delta).
    pub text: String,
    /// True only for the final delta.
    pub done: bool,
    /// Finish reason on the terminal delta (`stop`, `length`, …).
    pub finish_reason: Option<String>,
}

impl ChatDelta {
    /// A text fragment delta.
    pub fn text(s: impl Into<String>) -> Self {
        Self {
            text: s.into(),
            done: false,
            finish_reason: None,
        }
    }
    /// The terminal delta.
    pub fn done(reason: impl Into<String>) -> Self {
        Self {
            text: String::new(),
            done: true,
            finish_reason: Some(reason.into()),
        }
    }
}

/// Errors a [`ChatModel`] can surface mid-stream.
#[derive(Debug, thiserror::Error)]
pub enum ChatError {
    /// Transport / HTTP failure talking to the upstream model.
    #[error("model transport: {0}")]
    Transport(String),
    /// Upstream returned a non-success status.
    #[error("model http {status}: {body}")]
    Status {
        /// HTTP status code.
        status: u16,
        /// Response body (truncated by the caller if needed).
        body: String,
    },
}

/// A boxed stream of completion deltas.
pub type ChatStream = Pin<Box<dyn Stream<Item = Result<ChatDelta, ChatError>> + Send>>;

/// A pluggable chat model backing the gRPC `StreamChat` RPC.
pub trait ChatModel: Send + Sync {
    /// Begin streaming a completion for `req`. The returned stream is consumed
    /// lazily by the gRPC layer, so dropping it cancels generation.
    fn stream(&self, req: ChatRequest) -> ChatStream;
}

// ---------------------------------------------------------------------------
// EchoModel — hermetic reference
// ---------------------------------------------------------------------------

/// A no-network reference model: streams the prompt back, one word per delta,
/// then a terminal `stop`. Useful as a default and in tests.
#[derive(Debug, Clone, Default)]
pub struct EchoModel;

impl ChatModel for EchoModel {
    fn stream(&self, req: ChatRequest) -> ChatStream {
        Box::pin(async_stream::stream! {
            let words: Vec<String> = req.prompt.split_whitespace().map(String::from).collect();
            for (i, w) in words.iter().enumerate() {
                let text = if i == 0 { w.clone() } else { format!(" {w}") };
                yield Ok(ChatDelta::text(text));
            }
            yield Ok(ChatDelta::done("stop"));
        })
    }
}

// ---------------------------------------------------------------------------
// SSE decoding (pure, testable)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SseChunk {
    choices: Vec<SseChoice>,
}

#[derive(Deserialize)]
struct SseChoice {
    #[serde(default)]
    delta: SseDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize, Default)]
struct SseDelta {
    #[serde(default)]
    content: Option<String>,
}

/// Incremental decoder for OpenAI-style `text/event-stream` chat output.
///
/// Feed it raw byte chunks (which may split lines anywhere) with [`push`]; it
/// buffers partial lines and returns the [`ChatDelta`]s completed so far,
/// including a terminal delta when it sees `data: [DONE]`.
///
/// [`push`]: SseDecoder::push
#[derive(Default)]
pub struct SseDecoder {
    buf: String,
    finished: bool,
}

impl SseDecoder {
    /// A fresh decoder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a chunk of bytes; return any newly-completed deltas.
    pub fn push(&mut self, chunk: &str) -> Vec<ChatDelta> {
        self.buf.push_str(chunk);
        let mut out = Vec::new();
        while let Some(nl) = self.buf.find('\n') {
            let line: String = self.buf.drain(..=nl).collect();
            let line = line.trim_end();
            let Some(data) = line.strip_prefix("data:") else {
                continue; // ignore comments, event:, id:, blank lines
            };
            let data = data.trim();
            if data.is_empty() {
                continue;
            }
            if data == "[DONE]" {
                if !self.finished {
                    self.finished = true;
                    out.push(ChatDelta::done("stop"));
                }
                continue;
            }
            if let Ok(parsed) = serde_json::from_str::<SseChunk>(data) {
                if let Some(choice) = parsed.choices.into_iter().next() {
                    if let Some(text) = choice.delta.content {
                        if !text.is_empty() {
                            out.push(ChatDelta::text(text));
                        }
                    }
                    if let Some(reason) = choice.finish_reason {
                        if !self.finished {
                            self.finished = true;
                            out.push(ChatDelta::done(reason));
                        }
                    }
                }
            }
        }
        out
    }
}

/// Turn a stream of raw SSE byte chunks into a stream of [`ChatDelta`]s using
/// [`SseDecoder`]. Factored out so the decode path is testable with canned
/// bytes, no network. Guarantees a terminal `done` delta even if the upstream
/// closes without `[DONE]`.
pub fn deltas_from_byte_stream<S>(bytes: S) -> ChatStream
where
    S: Stream<Item = Result<Bytes, ChatError>> + Send + 'static,
{
    Box::pin(async_stream::stream! {
        let mut dec = SseDecoder::new();
        let mut emitted_done = false;
        futures_util::pin_mut!(bytes);
        while let Some(chunk) = bytes.next().await {
            let chunk = chunk?;
            let text = String::from_utf8_lossy(&chunk);
            for delta in dec.push(&text) {
                if delta.done { emitted_done = true; }
                yield Ok(delta);
            }
        }
        if !emitted_done {
            yield Ok(ChatDelta::done("stop"));
        }
    })
}

// ---------------------------------------------------------------------------
// OpenAiChatModel — real backend
// ---------------------------------------------------------------------------

/// Backs `StreamChat` with any OpenAI-compatible `/v1/chat/completions`
/// streaming endpoint.
///
/// The bearer token is private and never printed: `Debug` reports only whether
/// a key is set, so an accidental `{:?}` or a tracing span can't leak the
/// secret (same contract as `spine_gateway`'s `BearerConfig`).
#[derive(Clone)]
pub struct OpenAiChatModel {
    client: reqwest::Client,
    /// Base URL, e.g. `https://api.openai.com` or `http://localhost:9091`.
    pub base_url: String,
    /// Optional bearer token — private; set via [`OpenAiChatModel::with_api_key`].
    api_key: Option<String>,
}

impl std::fmt::Debug for OpenAiChatModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiChatModel")
            .field("base_url", &self.base_url)
            // Presence only — never the token bytes.
            .field("api_key", &self.api_key.as_ref().map(|_| "<set>"))
            .finish()
    }
}

impl OpenAiChatModel {
    /// Build a model pointed at `base_url` (no trailing slash needed).
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
            api_key: None,
        }
    }

    /// Set the bearer token. The token is stored privately and never logged.
    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Whether a bearer token is configured (without exposing it).
    pub fn has_api_key(&self) -> bool {
        self.api_key.is_some()
    }
}

impl ChatModel for OpenAiChatModel {
    fn stream(&self, req: ChatRequest) -> ChatStream {
        let url = format!("{}/v1/chat/completions", self.base_url.trim_end_matches('/'));
        let api_key = self.api_key.clone();
        let client = self.client.clone();
        let body = serde_json::json!({
            "model": req.model,
            "stream": true,
            "messages": [{ "role": "user", "content": req.prompt }],
        });

        let bytes = async_stream::stream! {
            let mut builder = client.post(&url).json(&body);
            if let Some(key) = &api_key {
                builder = builder.bearer_auth(key);
            }
            let resp = match builder.send().await {
                Ok(r) => r,
                Err(e) => { yield Err(ChatError::Transport(e.to_string())); return; }
            };
            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                // Cap the echoed upstream body so a large or sensitive error
                // response can't flow through unbounded.
                let body: String = resp.text().await.unwrap_or_default().chars().take(256).collect();
                yield Err(ChatError::Status { status, body });
                return;
            }
            let mut stream = resp.bytes_stream();
            while let Some(item) = stream.next().await {
                match item {
                    Ok(b) => yield Ok(b),
                    Err(e) => { yield Err(ChatError::Transport(e.to_string())); return; }
                }
            }
        };
        deltas_from_byte_stream(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_prints_the_api_key() {
        let m = OpenAiChatModel::new("https://api.example.com").with_api_key("sk-SECRET-token-123");
        let dbg = format!("{m:?}");
        assert!(!dbg.contains("sk-SECRET-token-123"), "api key leaked in Debug: {dbg}");
        assert!(dbg.contains("<set>"), "Debug should note a key is present");
        assert!(m.has_api_key());
        // No key configured -> Debug shows None, still no secret.
        let none = OpenAiChatModel::new("https://api.example.com");
        assert!(!none.has_api_key());
        assert!(format!("{none:?}").contains("None"));
    }

    #[tokio::test]
    async fn echo_model_streams_words_then_done() {
        let deltas: Vec<ChatDelta> = EchoModel
            .stream(ChatRequest { model: "m".into(), prompt: "alpha beta".into() })
            .map(|r| r.unwrap())
            .collect()
            .await;
        assert_eq!(deltas[0], ChatDelta::text("alpha"));
        assert_eq!(deltas[1], ChatDelta::text(" beta"));
        assert!(deltas[2].done);
        assert_eq!(deltas[2].finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn sse_decoder_handles_split_lines() {
        let mut dec = SseDecoder::new();
        // A data line split across two pushes, then a finish_reason, then DONE.
        let mut out = dec.push("data: {\"choices\":[{\"delta\":{\"content\":\"Hel");
        assert!(out.is_empty(), "partial line yields nothing yet");
        out.extend(dec.push("lo\"}}]}\n"));
        assert_eq!(out, vec![ChatDelta::text("Hello")]);

        let fin = dec.push("data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n");
        assert_eq!(fin, vec![ChatDelta::done("stop")]);

        // A trailing [DONE] after a finish_reason must not double-emit done.
        let done = dec.push("data: [DONE]\n");
        assert!(done.is_empty());
    }

    #[test]
    fn sse_decoder_ignores_comments_and_blanks() {
        let mut dec = SseDecoder::new();
        let out = dec.push(": keep-alive\n\nevent: ping\n");
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn deltas_from_byte_stream_decodes_canned_sse() {
        // Bytes deliberately split mid-token to exercise buffering.
        let chunks: Vec<Result<Bytes, ChatError>> = vec![
            Ok(Bytes::from("data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"}}]}\n")),
            Ok(Bytes::from("data: {\"choices\":[{\"delta\":{\"content\":\" the")),
            Ok(Bytes::from("re\"}}]}\n")),
            Ok(Bytes::from("data: [DONE]\n")),
        ];
        let stream = deltas_from_byte_stream(futures_util::stream::iter(chunks));
        let deltas: Vec<ChatDelta> = stream.map(|r| r.unwrap()).collect().await;
        assert_eq!(deltas[0], ChatDelta::text("Hi"));
        assert_eq!(deltas[1], ChatDelta::text(" there"));
        assert!(deltas[2].done);
    }

    #[tokio::test]
    async fn byte_stream_synthesizes_done_if_upstream_omits_it() {
        let chunks: Vec<Result<Bytes, ChatError>> = vec![Ok(Bytes::from(
            "data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n",
        ))];
        let deltas: Vec<ChatDelta> = deltas_from_byte_stream(futures_util::stream::iter(chunks))
            .map(|r| r.unwrap())
            .collect()
            .await;
        assert_eq!(deltas.len(), 2);
        assert!(deltas[1].done);
    }

    #[tokio::test]
    async fn transport_error_propagates() {
        let chunks: Vec<Result<Bytes, ChatError>> =
            vec![Err(ChatError::Transport("boom".into()))];
        let results: Vec<Result<ChatDelta, ChatError>> =
            deltas_from_byte_stream(futures_util::stream::iter(chunks))
                .collect()
                .await;
        assert!(matches!(results[0], Err(ChatError::Transport(_))));
    }
}
