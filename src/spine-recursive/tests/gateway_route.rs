//! The dispatchers' opt-in route through IronGate, held to the wire.
//!
//! A listener on a loopback port stands in for the gateway and records the one
//! request each dispatcher makes, answering as IronGate's OpenAI-compatible
//! chat API does. What is checked is SPINE's half of the contract: the path,
//! the gateway's token and never a provider's key, the body's shape and text,
//! that the answer comes back -- and that a local model server stays direct,
//! and a deployment that requires the gateway refuses to go without one.
//! Whether IronGate routes the request on is IronGate's suite.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::thread::JoinHandle;

use spine_recursive::gateway_route::Settings;
use spine_recursive::{
    AnthropicConfig, AnthropicDispatcher, OpenAiConfig, OpenAiDispatcher, SubLlmDispatcher,
};

const CHAT_ANSWER: &str = r#"{"choices":[{"message":{"role":"assistant","content":"forty-two"}}],"usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}}"#;

/// A request as the stand-in saw it.
struct Seen {
    line: String,
    headers: Vec<String>,
    body: serde_json::Value,
}

impl Seen {
    fn header(&self, name: &str) -> Option<String> {
        let prefix = format!("{}:", name.to_ascii_lowercase());
        self.headers
            .iter()
            .find(|h| h.to_ascii_lowercase().starts_with(&prefix))
            .map(|h| h[prefix.len()..].trim().to_string())
    }
}

/// Serve one request on a fresh loopback port; returns its base URL.
fn stand_in(answer: &'static str) -> (String, JoinHandle<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handle = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let (mut headers, mut length) = (Vec::new(), 0usize);
        loop {
            let mut h = String::new();
            reader.read_line(&mut h).unwrap();
            let h = h.trim_end().to_string();
            if h.is_empty() {
                break;
            }
            if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                length = v.trim().parse().unwrap();
            }
            headers.push(h);
        }
        let mut body = vec![0u8; length];
        reader.read_exact(&mut body).unwrap();
        let mut out = stream;
        write!(
            out,
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{answer}",
            answer.len()
        )
        .unwrap();
        Seen {
            line: line.trim_end().to_string(),
            headers,
            body: serde_json::from_slice(&body).unwrap(),
        }
    });
    (base, handle)
}

fn gateway(url: &str) -> Settings {
    Settings {
        url: Some(url.to_string()),
        token: Some("gate-token".into()),
        required: false,
    }
}

/// Never contacted: a remote provider the gateway stands in front of.
const REMOTE: &str = "https://provider.invalid/v1";

#[tokio::test]
async fn the_openai_dispatcher_goes_through_the_gateway_with_no_provider_key() {
    let (base, served) = stand_in(CHAT_ANSWER);
    let d = OpenAiDispatcher::with_gateway(
        OpenAiConfig {
            api_key: "provider-key".into(),
            model: "some-model".into(),
            base_url: REMOTE.into(),
            max_retries: 1,
            ..Default::default()
        },
        gateway(&format!("{base}/")),
    );
    let answer = d.dispatch("What is it?", "the context", 0).await.unwrap();
    let seen = served.join().unwrap();

    assert_eq!(answer, "forty-two");
    assert_eq!(d.total_tokens(), 7);
    assert_eq!(seen.line, "POST /v1/chat/completions HTTP/1.1");
    assert_eq!(
        seen.header("authorization").as_deref(),
        Some("Bearer gate-token")
    );
    assert!(
        !seen.headers.iter().any(|h| h.contains("provider-key")),
        "a provider key went to the gateway: {:?}",
        seen.headers
    );
    assert_eq!(seen.body["model"], "some-model");
    assert_eq!(
        seen.body["messages"][0]["content"],
        "Context:\nthe context\n\nQuestion/Task:\nWhat is it?"
    );
}

#[tokio::test]
async fn the_anthropic_dispatcher_is_translated_into_the_gateways_chat_shape() {
    let (base, served) = stand_in(CHAT_ANSWER);
    let d = AnthropicDispatcher::with_gateway(
        AnthropicConfig {
            api_key: "provider-key".into(),
            model: "claude-model".into(),
            max_tokens: 321,
            ..Default::default()
        },
        gateway(&base),
    );
    let answer = d.dispatch("Summarize.", "", 0).await.unwrap();
    let seen = served.join().unwrap();

    assert_eq!(answer, "forty-two");
    assert_eq!(d.call_count(), 1);
    assert_eq!(d.total_tokens(), 7);
    assert_eq!(seen.line, "POST /v1/chat/completions HTTP/1.1");
    assert_eq!(
        seen.header("authorization").as_deref(),
        Some("Bearer gate-token")
    );
    assert_eq!(
        seen.header("x-api-key"),
        None,
        "Anthropic's key header went to the gateway"
    );
    assert_eq!(seen.header("anthropic-version"), None);
    assert_eq!(
        seen.body,
        serde_json::json!({
            "model": "claude-model",
            "messages": [{ "role": "user", "content": "Summarize." }],
            "max_tokens": 321,
        }),
        "the same model, message and limit the direct call sends"
    );
}

/// A local model server never leaves the machine, so it stays direct -- with
/// its own key -- even when a gateway is configured.
#[tokio::test]
async fn a_local_model_server_stays_direct_with_a_gateway_configured() {
    let (base, served) = stand_in(CHAT_ANSWER);
    let d = OpenAiDispatcher::with_gateway(
        OpenAiConfig {
            api_key: "local-key".into(),
            base_url: format!("{base}/v1"),
            max_retries: 1,
            ..Default::default()
        },
        gateway("http://gateway.invalid:7700"),
    );
    assert_eq!(d.dispatch("q", "", 0).await.unwrap(), "forty-two");
    let seen = served.join().unwrap();
    assert_eq!(seen.line, "POST /v1/chat/completions HTTP/1.1");
    assert_eq!(
        seen.header("authorization").as_deref(),
        Some("Bearer local-key")
    );
}

#[tokio::test]
async fn requiring_the_gateway_without_one_refuses_before_any_call() {
    let required = Settings {
        required: true,
        ..Default::default()
    };
    let openai = OpenAiDispatcher::with_gateway(
        OpenAiConfig {
            api_key: "k".into(),
            base_url: REMOTE.into(),
            ..Default::default()
        },
        required.clone(),
    );
    let anthropic = AnthropicDispatcher::with_gateway(
        AnthropicConfig {
            api_key: "k".into(),
            ..Default::default()
        },
        required,
    );
    for (name, err) in [
        ("openai", openai.dispatch("q", "", 0).await.unwrap_err()),
        (
            "anthropic",
            anthropic.dispatch("q", "", 0).await.unwrap_err(),
        ),
    ] {
        let text = err.to_string();
        assert!(text.contains("SPINE_REQUIRE_GATEWAY"), "{name}: {text}");
        assert!(text.contains("IRONGATE_URL"), "{name}: {text}");
    }
    assert_eq!(anthropic.call_count(), 0);
}
