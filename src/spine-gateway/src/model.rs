//! Configurable OpenAI-compatible backend. Polling upstream bytes directly
//! preserves tool deltas and usage, applies backpressure, and drops the upstream
//! response when the downstream consumer cancels. No token queue is accumulated.

use axum::{
    body::Body,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use futures::StreamExt;
use serde_json::Value;
use spine_recursive::gateway_route::{Route, Settings};
use std::{sync::Arc, time::Duration};

#[derive(Clone)]
pub enum ChatBackend {
    Disabled,
    Demo,
    Remote {
        client: reqwest::Client,
        endpoint: String,
        token: Option<String>,
    },
}

impl ChatBackend {
    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_vars(|key| std::env::var(key).ok())
    }

    fn from_vars(var: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let base = var("SPINE_MODEL_URL").filter(|v| !v.trim().is_empty());
        let settings = Settings::from_vars(&var);
        let demo = var("SPINE_GATEWAY_DEMO").as_deref() == Some("1");
        anyhow::ensure!(
            !demo || (base.is_none() && settings.url.is_none() && !settings.required),
            "SPINE_GATEWAY_DEMO cannot be combined with a model or required gateway"
        );
        if demo {
            return Ok(Self::Demo);
        }
        if base.is_none() && settings.url.is_none() {
            anyhow::ensure!(
                !settings.required,
                "SPINE_REQUIRE_GATEWAY requires IRONGATE_URL"
            );
            return Ok(Self::Disabled);
        }
        let direct = base.unwrap_or_else(|| "https://api.openai.com/v1".into());
        let (endpoint, token) = match settings
            .route(&direct, "spine-gateway")
            .map_err(anyhow::Error::msg)?
        {
            Route::Direct => (
                format!("{}/chat/completions", direct.trim_end_matches('/')),
                var("SPINE_MODEL_TOKEN").filter(|v| !v.is_empty()),
            ),
            Route::Gateway { endpoint, token } => (endpoint, token),
        };
        let url = reqwest::Url::parse(&endpoint)?;
        anyhow::ensure!(
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "invalid model URL"
        );
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(300))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self::Remote {
            client,
            endpoint,
            token,
        })
    }

    async fn complete(&self, body: Value) -> Result<Response, (StatusCode, String)> {
        let fail = |status, message: &str| (status, message.to_owned());
        let Some(messages) = body.get("messages").and_then(Value::as_array) else {
            return Err(fail(StatusCode::BAD_REQUEST, "messages must be an array"));
        };
        if messages.is_empty() {
            return Err(fail(StatusCode::BAD_REQUEST, "messages cannot be empty"));
        }
        match self {
            Self::Disabled => Err(fail(StatusCode::SERVICE_UNAVAILABLE,
                "No model configured. Set SPINE_MODEL_URL or IRONGATE_URL; select SPINE_GATEWAY_DEMO=1 for echo.")),
            Self::Demo => {
                let req = serde_json::from_value(body)
                    .map_err(|_| fail(StatusCode::BAD_REQUEST, "invalid demo chat request"))?;
                super::agentic_sse::chat_completions_stream(Json(req)).await
            }
            Self::Remote { client, endpoint, token } => {
                if body.get("model").and_then(Value::as_str).is_none_or(str::is_empty) {
                    return Err(fail(StatusCode::BAD_REQUEST, "model is required"));
                }
                if body.get("stream").is_some_and(|v| !v.is_boolean()) {
                    return Err(fail(StatusCode::BAD_REQUEST, "stream must be a boolean"));
                }
                let streaming = body.get("stream").and_then(Value::as_bool).unwrap_or(false);
                let mut request = client.post(endpoint).json(&body);
                if let Some(token) = token { request = request.bearer_auth(token); }
                let response = request.send().await.map_err(|_| fail(StatusCode::BAD_GATEWAY, "model connection failed"))?;
                if !response.status().is_success() {
                    return Err(fail(StatusCode::BAD_GATEWAY, "model rejected the request"));
                }
                if streaming {
                    let is_sse = response.headers().get(reqwest::header::CONTENT_TYPE)
                        .and_then(|v| v.to_str().ok()).is_some_and(|v| v.starts_with("text/event-stream"));
                    if !is_sse { return Err(fail(StatusCode::BAD_GATEWAY, "model did not return SSE")); }
                    let bytes = response.bytes_stream().map(|chunk| chunk.map_err(|_| std::io::Error::other("model stream interrupted")));
                    Ok(Response::builder().header("content-type", "text/event-stream")
                        .header("cache-control", "no-cache").body(Body::from_stream(bytes)).unwrap())
                } else {
                    let mut bytes = Vec::new();
                    let mut stream = response.bytes_stream();
                    while let Some(chunk) = stream.next().await {
                        let chunk = chunk.map_err(|_| fail(StatusCode::BAD_GATEWAY, "model response interrupted"))?;
                        if bytes.len().saturating_add(chunk.len()) > 8 * 1024 * 1024 {
                            return Err(fail(StatusCode::BAD_GATEWAY, "model response too large"));
                        }
                        bytes.extend_from_slice(&chunk);
                    }
                    let value: Value = serde_json::from_slice(&bytes)
                        .map_err(|_| fail(StatusCode::BAD_GATEWAY, "model returned invalid JSON"))?;
                    Ok(Json(value).into_response())
                }
            }
        }
    }
}

pub async fn chat_completions(
    State(state): State<Arc<super::AppState>>,
    Json(body): Json<Value>,
) -> Result<Response, (StatusCode, String)> {
    state.chat_backend.complete(body).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn configuration_never_silently_selects_echo() {
        assert!(matches!(
            ChatBackend::from_vars(|_| None).unwrap(),
            ChatBackend::Disabled
        ));
        assert!(
            ChatBackend::from_vars(|k| (k == "SPINE_REQUIRE_GATEWAY").then(|| "1".into())).is_err()
        );
        assert!(matches!(
            ChatBackend::from_vars(|k| (k == "SPINE_GATEWAY_DEMO").then(|| "1".into())).unwrap(),
            ChatBackend::Demo
        ));
    }

    #[tokio::test]
    async fn an_unconfigured_model_returns_unavailable() {
        let result = ChatBackend::Disabled
            .complete(json!({"messages": [{"role":"user","content":"hello"}]}))
            .await;
        assert_eq!(result.unwrap_err().0, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn gateway_routing_uses_only_the_gateway_token_and_keeps_local_models_direct() {
        let configured = |key: &str| match key {
            "SPINE_MODEL_URL" => Some("https://provider.example/v1".into()),
            "SPINE_MODEL_TOKEN" => Some("provider-secret".into()),
            "IRONGATE_URL" => Some("http://127.0.0.1:7700".into()),
            "IRONGATE_TOKEN" => Some("gateway-secret".into()),
            "SPINE_REQUIRE_GATEWAY" => Some("1".into()),
            _ => None,
        };
        let ChatBackend::Remote {
            endpoint, token, ..
        } = ChatBackend::from_vars(configured).unwrap()
        else {
            panic!("no backend")
        };
        assert_eq!(endpoint, "http://127.0.0.1:7700/v1/chat/completions");
        assert_eq!(token.as_deref(), Some("gateway-secret"));
        let ChatBackend::Remote {
            endpoint, token, ..
        } = ChatBackend::from_vars(|key| {
            if key == "SPINE_MODEL_URL" {
                Some("http://127.0.0.1:9999/v1".into())
            } else {
                configured(key)
            }
        })
        .unwrap()
        else {
            panic!("no backend")
        };
        assert_eq!(endpoint, "http://127.0.0.1:9999/v1/chat/completions");
        assert_eq!(token.as_deref(), Some("provider-secret"));
        let settings = Settings::from_vars(configured);
        assert!(!format!("{settings:?}").contains("gateway-secret"));
        assert!(!format!(
            "{:?}",
            settings
                .route("https://provider.example/v1", "test")
                .unwrap()
        )
        .contains("gateway-secret"));
    }

    #[tokio::test]
    async fn dropping_the_downstream_response_closes_the_upstream_stream() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (closed_tx, closed_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            assert!(socket.read(&mut request).await.unwrap() > 0);
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n").await.unwrap();
            let chunk = format!("4000\r\n{}\r\n", "x".repeat(16384));
            loop {
                if socket.write_all(chunk.as_bytes()).await.is_err() {
                    let _ = closed_tx.send(());
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        });
        let base = format!("http://{addr}/v1");
        let backend =
            ChatBackend::from_vars(|key| (key == "SPINE_MODEL_URL").then(|| base.clone())).unwrap();
        let response = backend.complete(json!({"model":"mock","stream":true,"messages":[{"role":"user","content":"hello"}]})).await.unwrap();
        drop(response);
        tokio::time::timeout(Duration::from_secs(5), closed_rx)
            .await
            .unwrap()
            .unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn real_model_preserves_messages_tools_and_stream_bytes() {
        let request = json!({"model":"local","stream":true,"messages":[{"role":"system","content":"rules"},{"role":"user","content":"hello"}],"tools":[{"type":"function","function":{"name":"search"}}]});
        let expected = request.clone();
        let bytes = "data: {\"choices\":[{\"delta\":{\"content\":\"雪\"}}]}\n\ndata: [DONE]\n\n";
        let app = axum::Router::new().route(
            "/v1/chat/completions",
            axum::routing::post(move |Json(body): Json<Value>| {
                let expected = expected.clone();
                async move {
                    assert_eq!(body, expected);
                    ([("content-type", "text/event-stream")], bytes)
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let base = format!("http://{addr}/v1");
        let backend =
            ChatBackend::from_vars(|k| (k == "SPINE_MODEL_URL").then(|| base.clone())).unwrap();
        let response = backend.complete(request).await.unwrap();
        let result = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(&result[..], bytes.as_bytes());
        server.abort();
    }
}
