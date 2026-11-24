//! Neural codec endpoints: `POST /v1/embeddings` and `GET /v1/agentic/codecs`.
//!
//! Compiled only with the gateway's `research` feature. The codec types come
//! from `spine-protocol-research`; they are not part of the native wire. A
//! gateway built without the feature answers both routes with `503` (see
//! [`super::embeddings`] and [`super::codecs`]).

use axum::extract::Json;
use serde::Deserialize;
use serde_json::json;
use spine_protocol_research::{CodecRegistry, EmbeddingInput, EmbeddingRequest, TitansLatentCodec};
use std::sync::OnceLock;

/// Process-wide codec registry. Lazily initialised on first access, and
/// pre-populated with the default `TitansLatentCodec` (256-dim, f32).
/// Production deployments may register more codecs through
/// [`CodecRegistry::register`] at start up; once you hold the registry,
/// registration is concurrent-safe.
pub fn registry() -> &'static CodecRegistry {
    static REG: OnceLock<CodecRegistry> = OnceLock::new();
    REG.get_or_init(|| {
        let r = CodecRegistry::new();
        r.register(std::sync::Arc::new(TitansLatentCodec::new(256)));
        r
    })
}

/// OpenAI-compatible request: `{ "input": <string|[string]>, "model": "..." }`.
#[derive(Debug, Deserialize)]
pub struct EmbeddingsHttpReq {
    pub input: serde_json::Value,
    #[serde(default = "default_embed_model")]
    pub model: String,
}

/// The model used when a request omits `model`: the Titans codec.
fn default_embed_model() -> String {
    "spine:codec/titans/v1@dim=256,dtype=f32".into()
}

/// `POST /v1/embeddings` — OpenAI-shaped wrapper around [`EmbeddingRequest`].
/// Internally:
///
/// 1. Reshape the OpenAI payload into [`EmbeddingRequest`].
/// 2. Resolve the codec via the process registry.
/// 3. Encode every input element to an encoded frame.
/// 4. Project the f32 latent back into the OpenAI response shape so the
///    existing client SDK reads `data[i].embedding` as a `[f32]`.
///
/// **Privacy note**: `skip_all` keeps embedding input out of tracing
/// spans. The handler logs no input content.
#[tracing::instrument(skip_all, fields(model = %req.model))]
pub async fn embeddings(
    Json(req): Json<EmbeddingsHttpReq>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    embeddings_with(registry(), req)
}

/// [`embeddings`] against an explicit registry. Answers `503 no codecs
/// registered` when `reg` is empty.
fn embeddings_with(
    reg: &CodecRegistry,
    req: EmbeddingsHttpReq,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    use axum::http::StatusCode;

    // Map OpenAI's input to a SPINE EmbeddingInput.
    let input = match &req.input {
        serde_json::Value::String(s) => EmbeddingInput::Text(s.clone()),
        serde_json::Value::Array(arr) => {
            let texts: Result<Vec<String>, _> = arr
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(|s| s.to_string())
                        .ok_or("array elements must be strings")
                })
                .collect();
            match texts {
                Ok(t) => EmbeddingInput::Texts(t),
                Err(e) => return Err((StatusCode::BAD_REQUEST, e.to_string())),
            }
        }
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "input must be a string or array of strings".into(),
            ))
        }
    };

    let spine_req = EmbeddingRequest {
        id: uuid::Uuid::new_v4().to_string(),
        input,
        codec: Some(req.model.clone()),
        trace: None,
    };

    // Resolve codec — fall back to the registry's first codec when the
    // client asked for one we don't have (keeps existing OpenAI SDK
    // smoke tests working with `text-embedding-3-large`).
    let codec_id = if reg.get(&req.model).is_some() {
        req.model.clone()
    } else {
        reg.ids().first().cloned().ok_or((
            StatusCode::SERVICE_UNAVAILABLE,
            "no codecs registered".into(),
        ))?
    };

    let codec = reg.get(&codec_id).expect("just resolved");

    // Encode each input.
    let texts: Vec<String> = match &spine_req.input {
        EmbeddingInput::Text(s) => vec![s.clone()],
        EmbeddingInput::Texts(v) => v.clone(),
        EmbeddingInput::Encoded(_) => {
            return Err((
                StatusCode::BAD_REQUEST,
                "Encoded input not supported by this gateway endpoint; use /v1/embeddings/raw"
                    .into(),
            ))
        }
    };

    let mut data = Vec::with_capacity(texts.len());
    let mut total_input_bytes: u64 = 0;
    for (i, text) in texts.iter().enumerate() {
        let frame = match codec.encode(text.as_bytes()) {
            Ok(f) => f,
            Err(e) => {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("encode failed: {e}"),
                ))
            }
        };
        total_input_bytes += text.len() as u64;
        // Project f32 LE bytes back into a JSON array of numbers so the
        // OpenAI SDK reads `data[i].embedding` natively.
        let mut floats = Vec::with_capacity(frame.data.len() / 4);
        for chunk in frame.data.as_chunks::<4>().0 {
            floats.push(f32::from_le_bytes(*chunk));
        }
        data.push(json!({
            "object": "embedding",
            "embedding": floats,
            "index": i,
        }));
    }

    Ok(Json(json!({
        "object": "list",
        "data": data,
        "model": codec_id,
        "usage": {
            "prompt_tokens": total_input_bytes,
            "total_tokens": total_input_bytes,
        }
    })))
}

/// `GET /v1/agentic/codecs` — emits the gateway's codec advertisement (every
/// registered codec). Lets HTTP clients discover what encoder/decoder pairs
/// are available without learning SPINE binary frames.
#[tracing::instrument]
pub async fn codecs() -> Json<serde_json::Value> {
    let ad = registry().advertise("spine-gateway", "static-gateway");
    Json(serde_json::to_value(&ad).expect("CodecAdvertisement is serializable"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use spine_protocol_research::{
        CodecDescriptor, CodecDirection, CodecError, DType, EncodedFrame, EncodedMetadata,
        Modality, NeuralCodec,
    };

    #[tokio::test]
    async fn embeddings_endpoint_round_trip_via_titans() {
        // Single string input.
        let req = Json(EmbeddingsHttpReq {
            input: json!("the quick brown fox"),
            model: "spine:codec/titans/v1@dim=256,dtype=f32".into(),
        });
        let resp = embeddings(req).await.expect("ok").0;
        assert_eq!(resp["object"], "list");
        assert_eq!(resp["model"], "spine:codec/titans/v1@dim=256,dtype=f32");
        assert_eq!(resp["data"].as_array().unwrap().len(), 1);
        let embedding = resp["data"][0]["embedding"].as_array().unwrap();
        assert_eq!(embedding.len(), 256);
        // Every element is a finite f32.
        for v in embedding {
            let f = v.as_f64().expect("finite number");
            assert!(f.is_finite(), "embedding had NaN/Inf");
        }
        // Usage is reported by source byte length, not token count
        // (we don't have a tokeniser at this layer).
        assert_eq!(
            resp["usage"]["prompt_tokens"].as_u64().unwrap(),
            "the quick brown fox".len() as u64
        );
    }

    #[tokio::test]
    async fn embeddings_endpoint_handles_array_input() {
        let req = Json(EmbeddingsHttpReq {
            input: json!(["a", "b", "c"]),
            model: "spine:codec/titans/v1@dim=256,dtype=f32".into(),
        });
        let resp = embeddings(req).await.expect("ok").0;
        let data = resp["data"].as_array().unwrap();
        assert_eq!(data.len(), 3);
        // Indices are sequential.
        for (i, item) in data.iter().enumerate() {
            assert_eq!(item["index"].as_u64().unwrap(), i as u64);
        }
    }

    #[tokio::test]
    async fn embeddings_endpoint_falls_back_to_registry_default() {
        // An OpenAI-style model name the gateway doesn't know — should
        // fall back to whatever the registry has (Titans by default).
        let req = Json(EmbeddingsHttpReq {
            input: json!("hi"),
            model: "text-embedding-3-large".into(),
        });
        let resp = embeddings(req).await.expect("ok").0;
        assert!(resp["model"].as_str().unwrap().starts_with("spine:codec/"));
    }

    #[tokio::test]
    async fn embeddings_endpoint_rejects_bad_input_shape() {
        let req = Json(EmbeddingsHttpReq {
            input: json!(42),
            model: default_embed_model(),
        });
        let err = embeddings(req).await.unwrap_err();
        assert_eq!(err.0, axum::http::StatusCode::BAD_REQUEST);
    }

    /// Tiny codec that maps every input to four little-endian f32s.
    struct TestCodec;

    impl NeuralCodec for TestCodec {
        fn id(&self) -> &str {
            "test:codec/fixed@dim=4"
        }

        fn describe(&self) -> CodecDescriptor {
            CodecDescriptor {
                id: self.id().into(),
                description: "fixed test codec".into(),
                direction: CodecDirection::Encode,
                modality: Modality::Text,
                embedding_dim: Some(4),
                vocab_size: None,
                dtype: DType::F32,
                semantic_embedding: None,
            }
        }

        fn encode(&self, input: &[u8]) -> Result<EncodedFrame, CodecError> {
            let data: Vec<u8> = [1.0f32, 2.0, 3.0, input.len() as f32]
                .iter()
                .flat_map(|f| f.to_le_bytes())
                .collect();
            Ok(EncodedFrame {
                codec: self.id().into(),
                variant: None,
                data,
                metadata: EncodedMetadata {
                    modality: Modality::Embedding,
                    shape: vec![4],
                    dtype: DType::F32,
                    original_len: None,
                    source_hash: None,
                },
                trace: None,
            })
        }

        fn decode(&self, _frame: &EncodedFrame) -> Result<Vec<u8>, CodecError> {
            Err(CodecError::EncodeOnly(self.id().into()))
        }
    }

    #[test]
    fn embeddings_with_explicit_registry() {
        let req = |model: &str| EmbeddingsHttpReq {
            input: json!("abc"),
            model: model.into(),
        };

        // An empty registry is a 503, not a panic.
        let empty = CodecRegistry::new();
        let err = embeddings_with(&empty, req("anything")).unwrap_err();
        assert_eq!(err.0, axum::http::StatusCode::SERVICE_UNAVAILABLE);

        // A registered codec serves its own id, and unknown ids fall back to it.
        let reg = CodecRegistry::new();
        reg.register(std::sync::Arc::new(TestCodec));
        for model in ["test:codec/fixed@dim=4", "", "text-embedding-3-large"] {
            let resp = embeddings_with(&reg, req(model)).expect("ok").0;
            assert_eq!(resp["model"], "test:codec/fixed@dim=4");
            assert_eq!(resp["data"][0]["embedding"], json!([1.0, 2.0, 3.0, 3.0]));
        }
    }

    #[tokio::test]
    async fn codecs_endpoint_returns_advertisement() {
        let ad = codecs().await.0;
        assert_eq!(ad["agent_id"], "spine-gateway");
        let list = ad["codecs"].as_array().unwrap();
        assert!(!list.is_empty(), "registry should have at least Titans");
        let ids: Vec<&str> = list.iter().filter_map(|c| c["id"].as_str()).collect();
        assert!(
            ids.iter().any(|id| id.contains("titans")),
            "default Titans codec missing: {ids:?}"
        );
    }
}
