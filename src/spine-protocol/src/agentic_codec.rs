//! Neural encoder-decoder protocols.
//!
//! Text on the wire is a lowest-common-denominator. Modern agents already
//! produce latents — embeddings, hidden states, key/value caches, image
//! patches, audio frames — and forcing every hop to round-trip back to
//! tokens throws away both bandwidth and signal. SPINE makes the latent
//! form a first-class payload.
//!
//! The contract has four parts, mirrored against the four families in
//! [`crate::agentic`]:
//!
//! 1. **Self-describing payload** ([`EncodedFrame`] + [`EncodedMetadata`])
//!    — every latent carries the codec id, shape, dtype, and modality so
//!    the receiver knows how to reconstruct or forward it without an
//!    out-of-band contract.
//!
//! 2. **Codec discovery** ([`CodecDescriptor`] + [`CodecAdvertisement`])
//!    — peers advertise the encoders/decoders they speak, in the same
//!    shape as capability advertisements. Selection can be exact,
//!    by-modality, or by semantic similarity over an optional embedding.
//!
//! 3. **Codec negotiation** ([`CodecNegotiation`]) — when two peers
//!    don't already agree, either side can offer a ranked list and the
//!    other side picks one. Falls out naturally over the existing
//!    [`crate::Message`] dispatch.
//!
//! 4. **Decoder hints** ([`DecodeHints`]) — sampling parameters
//!    (temperature, top-p/k, stop sequences, seed, repetition penalty)
//!    travel alongside the request that asks an autoregressive decoder
//!    to produce output. Mirrors the OpenAI / Anthropic surface so an
//!    existing client's knobs map 1:1.
//!
//! A minimal runtime — [`NeuralCodec`] trait plus [`CodecRegistry`] —
//! lives below the types so encoders can be registered, looked up, and
//! invoked symmetrically on either side of a connection.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::agentic::TraceContext;

// =============================================================================
// Modality, dtype, direction taxonomies
// =============================================================================

/// What kind of signal an encoded payload represents. Codecs declare the
/// modalities they handle so receivers can route to the right pipeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(tag = "kind", content = "name", rename_all = "snake_case")]
pub enum Modality {
    Text,
    Image,
    Audio,
    Video,
    /// A pre-computed vector (e.g. sentence embedding, image embedding).
    Embedding,
    /// Hidden state of a model (e.g. K/V cache, last hidden layer).
    HiddenState,
    /// Mixed-modality payload (e.g. tokenised text + image patches).
    Multimodal,
    /// Caller-defined. Use sparingly; receivers will likely refuse.
    Other(String),
}

/// Numeric type of the elements in an encoded buffer. Matches the
/// common ML inference taxonomy.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DType {
    F32,
    F16,
    BF16,
    I8,
    U8,
    I16,
    I32,
    /// 4-bit quantised (two values per byte).
    Q4,
    /// 8-bit quantised.
    Q8,
}

impl DType {
    /// Bytes per element. For sub-byte types this is the nominal pair
    /// size (Q4 returns 1 — the receiver must know it packs two values
    /// per byte).
    pub fn bytes_per_element(&self) -> usize {
        match self {
            DType::F32 | DType::I32 => 4,
            DType::F16 | DType::BF16 | DType::I16 => 2,
            DType::I8 | DType::U8 | DType::Q8 => 1,
            DType::Q4 => 1, // two packed values per byte
        }
    }
}

/// Whether a codec encodes, decodes, or does both (most do both).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CodecDirection {
    Encode,
    Decode,
    Both,
}

// =============================================================================
// Encoded payload
// =============================================================================

/// One self-describing latent. Drop this into a [`crate::Message`] (via
/// [`crate::Message::Encoded`]) and the receiver can validate, decode, or
/// forward without an out-of-band schema.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EncodedFrame {
    /// Codec identifier — stable URI like
    /// `"spine:codec/titans/v1@dim=256,dtype=f32"`. The same identifier
    /// must appear in the producer's [`CodecAdvertisement`].
    pub codec: String,
    /// Optional sub-revision (e.g. shard id, layer index, head id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    /// Raw byte payload. Interpretation defined by `metadata.dtype` and
    /// `metadata.shape`.
    pub data: Vec<u8>,
    /// Inline self-description.
    pub metadata: EncodedMetadata,
    /// Optional W3C trace context.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<TraceContext>,
}

/// What's actually inside [`EncodedFrame::data`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EncodedMetadata {
    pub modality: Modality,
    /// Logical shape, e.g. `[seq_len, embed_dim]` or `[batch, channels,
    /// h, w]`. Empty means scalar / opaque.
    #[serde(default)]
    pub shape: Vec<u32>,
    pub dtype: DType,
    /// Optional original byte length before encoding — useful for
    /// compression-style codecs that want to validate decoded output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_len: Option<u64>,
    /// Optional content hash of the source (e.g. SHA-256 of the input
    /// text or image bytes). Lets a receiver cache + dedup encoded
    /// outputs across requests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<[u8; 32]>,
}

impl EncodedFrame {
    /// Number of logical elements implied by `shape`. Returns 0 for a
    /// scalar / opaque payload.
    pub fn element_count(&self) -> u64 {
        if self.metadata.shape.is_empty() {
            0
        } else {
            self.metadata.shape.iter().map(|d| *d as u64).product()
        }
    }

    /// Expected byte length for the declared `shape × dtype`. Useful for
    /// quick sanity checks before decoding.
    pub fn expected_bytes(&self) -> u64 {
        self.element_count() * self.metadata.dtype.bytes_per_element() as u64
    }

    /// True when the payload's byte length matches `expected_bytes()`.
    /// Codecs that pack (Q4) or compress should produce `false` here —
    /// they're responsible for their own length contract.
    pub fn declared_size_consistent(&self) -> bool {
        self.expected_bytes() as usize == self.data.len()
    }
}

// =============================================================================
// Codec discovery & negotiation
// =============================================================================

/// One codec advertised by a peer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodecDescriptor {
    /// Stable codec id (matches [`EncodedFrame::codec`]).
    pub id: String,
    pub description: String,
    pub direction: CodecDirection,
    pub modality: Modality,
    /// Output dimension for encoders / decoders that produce a fixed-
    /// width embedding.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding_dim: Option<u32>,
    /// Vocabulary size for token-level codecs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vocab_size: Option<u32>,
    /// Native dtype the codec emits / accepts.
    pub dtype: DType,
    /// Optional semantic embedding so peers can match by similarity
    /// rather than by exact id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_embedding: Option<Vec<f32>>,
}

/// What a peer can speak. Returned in response to a `CapabilityQuery`
/// with selector `Exact("spine:codecs")` or as an unsolicited push.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodecAdvertisement {
    pub id: String,
    pub agent_id: String,
    pub codecs: Vec<CodecDescriptor>,
}

/// Offer / select handshake. Either side sends `offered` with a ranked
/// list; the responder echoes the same `id` with `accepted` populated.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CodecNegotiation {
    pub id: String,
    /// Ranked list of codec ids the offerer prefers, best-first.
    pub offered: Vec<String>,
    /// Set in the response. `None` means "no overlap; fall back to
    /// plain text".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted: Option<String>,
    /// Optional reason if `accepted` is `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

// =============================================================================
// Decoder hints (sampling parameters)
// =============================================================================

/// Sampling parameters for an autoregressive decoder. Travels alongside
/// the request that asks a model to produce output. Every field is
/// optional so that the decoder's own defaults survive when the caller
/// doesn't care.
///
/// Field names match the OpenAI / Anthropic surface, so an existing
/// client SDK round-trips through the gateway without translation.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DecodeHints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repetition_penalty: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence_penalty: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frequency_penalty: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stop_sequences: Vec<String>,
    /// Deterministic seed. Same seed + same inputs + same model →
    /// same outputs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

// =============================================================================
// Embedding endpoint (mirrors OpenAI /v1/embeddings)
// =============================================================================

/// "Embed this for me." A tool-call-shaped request that asks a peer to
/// run an encoder over `input` and return the latent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmbeddingRequest {
    pub id: String,
    /// What to embed. UTF-8 text is the common case; an
    /// already-encoded payload signals "transcode this for me".
    pub input: EmbeddingInput,
    /// Preferred codec. If `None`, the responder picks its default and
    /// echoes the choice back in [`EmbeddingResponse::codec`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<TraceContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EmbeddingInput {
    Text(String),
    Texts(Vec<String>),
    /// Re-encode this. Useful for cross-codec translation.
    Encoded(EncodedFrame),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmbeddingResponse {
    pub id: String,
    /// Which codec produced these embeddings (the responder's choice).
    pub codec: String,
    /// One frame per input element. For `Text` / single-encoded input
    /// this is length 1.
    pub embeddings: Vec<EncodedFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<TraceContext>,
}

// =============================================================================
// NeuralCodec runtime
// =============================================================================

#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("codec `{0}` not found in registry")]
    UnknownCodec(String),
    #[error("input shape/dtype mismatch: expected {expected}, got {got}")]
    ShapeMismatch { expected: String, got: String },
    #[error("encode failed: {0}")]
    Encode(String),
    #[error("decode failed: {0}")]
    Decode(String),
    #[error("codec `{0}` is encode-only")]
    EncodeOnly(String),
    #[error("codec `{0}` is decode-only")]
    DecodeOnly(String),
}

/// Symmetric encoder/decoder contract. Implementations live close to
/// the model they wrap (e.g. a Titans codec near `spine-neural`'s
/// encoder, a tokeniser codec inside an LLM runtime crate).
pub trait NeuralCodec: Send + Sync {
    /// Stable identifier matching [`CodecDescriptor::id`] and
    /// [`EncodedFrame::codec`].
    fn id(&self) -> &str;

    /// Self-description for advertisement.
    fn describe(&self) -> CodecDescriptor;

    /// Encode raw bytes into a self-describing frame. The bytes are
    /// modality-dependent: UTF-8 for text codecs, packed pixels for
    /// image codecs, etc.
    fn encode(&self, input: &[u8]) -> Result<EncodedFrame, CodecError>;

    /// Decode a frame back to raw bytes. Receivers that don't speak
    /// `frame.codec` should look it up by id in the [`CodecRegistry`].
    fn decode(&self, frame: &EncodedFrame) -> Result<Vec<u8>, CodecError>;
}

/// Thread-safe map of codec id → implementation. Populated by each
/// peer at startup; queried whenever an [`EncodedFrame`] arrives or a
/// negotiation needs to be answered.
#[derive(Default, Clone)]
pub struct CodecRegistry {
    codecs: Arc<parking_lot::RwLock<HashMap<String, Arc<dyn NeuralCodec>>>>,
}

impl CodecRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, codec: Arc<dyn NeuralCodec>) {
        let id = codec.id().to_string();
        self.codecs.write().insert(id, codec);
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn NeuralCodec>> {
        self.codecs.read().get(id).cloned()
    }

    /// All registered codec ids, sorted lexically (stable order for
    /// advertisement payloads).
    pub fn ids(&self) -> Vec<String> {
        let mut v: Vec<String> = self.codecs.read().keys().cloned().collect();
        v.sort();
        v
    }

    /// Build a [`CodecAdvertisement`] for everything currently registered.
    pub fn advertise(&self, agent_id: impl Into<String>, ad_id: impl Into<String>) -> CodecAdvertisement {
        let codecs = self
            .codecs
            .read()
            .values()
            .map(|c| c.describe())
            .collect();
        CodecAdvertisement {
            id: ad_id.into(),
            agent_id: agent_id.into(),
            codecs,
        }
    }

    /// First overlap between `offered` and the registry. Used to answer
    /// a [`CodecNegotiation`].
    pub fn pick_first(&self, offered: &[String]) -> Option<String> {
        let guard = self.codecs.read();
        offered.iter().find(|id| guard.contains_key(*id)).cloned()
    }

    pub fn encode(&self, codec_id: &str, input: &[u8]) -> Result<EncodedFrame, CodecError> {
        let codec = self
            .get(codec_id)
            .ok_or_else(|| CodecError::UnknownCodec(codec_id.to_string()))?;
        codec.encode(input)
    }

    pub fn decode(&self, frame: &EncodedFrame) -> Result<Vec<u8>, CodecError> {
        let codec = self
            .get(&frame.codec)
            .ok_or_else(|| CodecError::UnknownCodec(frame.codec.clone()))?;
        codec.decode(frame)
    }
}

// =============================================================================
// TitansLatentCodec — concrete bridge to spine-neural's encoder
// =============================================================================

/// Wraps [`spine_neural::NeuralLatentEncoder`] so the existing Titans /
/// MIRAS pipelines speak the [`NeuralCodec`] contract. Round-trip is
/// real: text is encoded via the neural projector, embedded into a
/// fixed-width latent, and the byte payload is the raw f32 buffer.
///
/// ## Statelessness (safe by default)
///
/// The underlying `NeuralLatentEncoder` is stateful — it accumulates a
/// `message_history` buffer that influences subsequent encodings via
/// the moving-target morph and threads its PRNG through every call. In
/// a shared / multi-tenant deployment (e.g. the gateway's
/// `/v1/embeddings`) that would let request A's content influence
/// request B's embedding — a cross-tenant data leak.
///
/// To make that **impossible by default**, every [`Self::encode`] call
/// resets the encoder's state via
/// [`spine_neural::NeuralLatentEncoder::reset_state`] using the codec's
/// stored seed. The codec is therefore safe to register in a process-
/// wide [`CodecRegistry`] and call from any number of concurrent
/// requests; same input → same output regardless of history.
///
/// Callers who specifically want context-aware encoding (e.g. a single
/// long agent session) should construct an isolated codec via
/// [`Self::stateful`] and use it from one thread.
pub struct TitansLatentCodec {
    id: String,
    encoder: parking_lot::Mutex<spine_neural::NeuralLatentEncoder>,
    embed_dim: u32,
    seed: u64,
    /// When true, every encode resets the wrapped encoder's history +
    /// PRNG. Default for any codec built via [`Self::new`] or
    /// [`Self::with_dims`].
    stateless: bool,
}

impl TitansLatentCodec {
    /// Build the default Titans codec with the given output dimension.
    /// `input_dim` is the projector's input width — text bytes are
    /// normalised + padded/truncated to fit before encoding. The
    /// returned codec is **stateless**: safe to register in a shared
    /// registry and call from concurrent requests.
    pub fn new(embed_dim: usize) -> Self {
        Self::with_dims(embed_dim, embed_dim, 4, 0xC0DEC)
    }

    /// Full stateless constructor: control the projector size,
    /// attention head count, and PRNG seed.
    pub fn with_dims(input_dim: usize, embed_dim: usize, heads: usize, seed: u64) -> Self {
        Self::build(input_dim, embed_dim, heads, seed, true)
    }

    /// **Context-aware** Titans codec — keeps `message_history` and
    /// PRNG state across calls. Only use this for a single agent
    /// session you control end-to-end. Do **not** register in a shared
    /// [`CodecRegistry`] that might serve more than one tenant; that
    /// would leak one user's content into another's embeddings via
    /// the moving-target morph.
    pub fn stateful(input_dim: usize, embed_dim: usize, heads: usize, seed: u64) -> Self {
        Self::build(input_dim, embed_dim, heads, seed, false)
    }

    fn build(input_dim: usize, embed_dim: usize, heads: usize, seed: u64, stateless: bool) -> Self {
        let encoder = spine_neural::NeuralLatentEncoder::new(
            input_dim,
            embed_dim,
            &[embed_dim * 2],
            heads,
            seed,
        );
        Self {
            id: format!("spine:codec/titans/v1@dim={embed_dim},dtype=f32"),
            encoder: parking_lot::Mutex::new(encoder),
            embed_dim: embed_dim as u32,
            seed,
            stateless,
        }
    }
}

impl NeuralCodec for TitansLatentCodec {
    fn id(&self) -> &str {
        &self.id
    }

    fn describe(&self) -> CodecDescriptor {
        CodecDescriptor {
            id: self.id.clone(),
            description: "Titans Neural Long-Term Memory encoder (text → fixed-width latent)"
                .into(),
            direction: CodecDirection::Encode,
            modality: Modality::Text,
            embedding_dim: Some(self.embed_dim),
            vocab_size: None,
            dtype: DType::F32,
            semantic_embedding: None,
        }
    }

    fn encode(&self, input: &[u8]) -> Result<EncodedFrame, CodecError> {
        // Real Titans encoder mutates internal history per call; lock
        // for thread-safety. Output is a Vec<f32> of length embed_dim.
        let latent: Vec<f32> = {
            let mut guard = self.encoder.lock();
            if self.stateless {
                // Reset history + re-seed PRNG so the prior call's
                // content cannot influence this one. Cheap relative to
                // the encode itself (just `Vec::clear` + StdRng init).
                guard.reset_state(self.seed);
            }
            guard.encode(input)
        };
        // Pack f32s into bytes (little-endian).
        let mut bytes = Vec::with_capacity(latent.len() * 4);
        for v in &latent {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        // Tag a source hash so receivers can dedup repeated encodings.
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(input);
        let mut source_hash = [0u8; 32];
        source_hash.copy_from_slice(&h.finalize());
        Ok(EncodedFrame {
            codec: self.id.clone(),
            variant: None,
            data: bytes,
            metadata: EncodedMetadata {
                modality: Modality::Embedding,
                shape: vec![latent.len() as u32],
                dtype: DType::F32,
                original_len: Some(input.len() as u64),
                source_hash: Some(source_hash),
            },
            trace: None,
        })
    }

    fn decode(&self, _frame: &EncodedFrame) -> Result<Vec<u8>, CodecError> {
        // The Titans VAE+morph pipeline is lossy on the text axis and
        // decoding requires the per-message morph_seed which is not
        // carried in the public frame. Surface this honestly rather
        // than returning garbage — receivers that need a round-trip
        // should pick a sequence codec.
        Err(CodecError::EncodeOnly(self.id.clone()))
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn json_round_trip<T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug>(
        v: &T,
    ) {
        let bytes = serde_json::to_vec(v).expect("encode");
        let back: T = serde_json::from_slice(&bytes).expect("decode");
        assert_eq!(v, &back);
    }

    #[test]
    fn modality_round_trip() {
        json_round_trip(&Modality::Text);
        json_round_trip(&Modality::Image);
        json_round_trip(&Modality::Embedding);
        json_round_trip(&Modality::HiddenState);
        json_round_trip(&Modality::Other("brain-mri-volume".into()));
    }

    #[test]
    fn dtype_size_table() {
        assert_eq!(DType::F32.bytes_per_element(), 4);
        assert_eq!(DType::F16.bytes_per_element(), 2);
        assert_eq!(DType::BF16.bytes_per_element(), 2);
        assert_eq!(DType::I8.bytes_per_element(), 1);
        assert_eq!(DType::Q4.bytes_per_element(), 1); // packed pair
    }

    #[test]
    fn encoded_frame_round_trip() {
        let frame = EncodedFrame {
            codec: "spine:codec/test/v1".into(),
            variant: Some("layer=12".into()),
            data: vec![1, 2, 3, 4, 5, 6, 7, 8],
            metadata: EncodedMetadata {
                modality: Modality::Embedding,
                shape: vec![2, 4],
                dtype: DType::U8,
                original_len: Some(8),
                source_hash: Some([0xAA; 32]),
            },
            trace: None,
        };
        json_round_trip(&frame);
        assert_eq!(frame.element_count(), 8);
        assert_eq!(frame.expected_bytes(), 8);
        assert!(frame.declared_size_consistent());
    }

    #[test]
    fn encoded_frame_detects_size_mismatch() {
        // 2×4 f32 should be 32 bytes, but only 16 supplied.
        let f = EncodedFrame {
            codec: "x".into(),
            variant: None,
            data: vec![0; 16],
            metadata: EncodedMetadata {
                modality: Modality::Embedding,
                shape: vec![2, 4],
                dtype: DType::F32,
                original_len: None,
                source_hash: None,
            },
            trace: None,
        };
        assert!(!f.declared_size_consistent());
        assert_eq!(f.expected_bytes(), 32);
    }

    #[test]
    fn codec_descriptor_round_trip() {
        let d = CodecDescriptor {
            id: "spine:codec/titans/v1@dim=256,dtype=f32".into(),
            description: "Titans projector".into(),
            direction: CodecDirection::Encode,
            modality: Modality::Text,
            embedding_dim: Some(256),
            vocab_size: None,
            dtype: DType::F32,
            semantic_embedding: Some(vec![0.1, 0.2, 0.3]),
        };
        json_round_trip(&d);
    }

    #[test]
    fn codec_advertisement_round_trip() {
        let ad = CodecAdvertisement {
            id: "ad1".into(),
            agent_id: "did:spine:peer-a".into(),
            codecs: vec![CodecDescriptor {
                id: "x".into(),
                description: "test".into(),
                direction: CodecDirection::Both,
                modality: Modality::Multimodal,
                embedding_dim: Some(128),
                vocab_size: Some(32_000),
                dtype: DType::BF16,
                semantic_embedding: None,
            }],
        };
        json_round_trip(&ad);
    }

    #[test]
    fn codec_negotiation_round_trip() {
        let req = CodecNegotiation {
            id: "neg1".into(),
            offered: vec![
                "spine:codec/titans/v1@dim=256,dtype=f32".into(),
                "openai:text-embedding-3-large".into(),
            ],
            accepted: None,
            reason: None,
        };
        json_round_trip(&req);

        let resp = CodecNegotiation {
            id: "neg1".into(),
            offered: req.offered.clone(),
            accepted: Some("openai:text-embedding-3-large".into()),
            reason: None,
        };
        json_round_trip(&resp);

        let no_overlap = CodecNegotiation {
            id: "neg1".into(),
            offered: req.offered.clone(),
            accepted: None,
            reason: Some("no overlap; falling back to plain text".into()),
        };
        json_round_trip(&no_overlap);
    }

    #[test]
    fn decode_hints_partial_round_trip() {
        let h = DecodeHints {
            temperature: Some(0.7),
            top_p: Some(0.95),
            max_tokens: Some(2048),
            stop_sequences: vec!["</tool>".into(), "USER:".into()],
            seed: Some(42),
            ..Default::default()
        };
        json_round_trip(&h);
        // Default DecodeHints serializes with no fields except the empty
        // stop_sequences vector skipped — make sure the empty case is
        // tiny on the wire.
        let empty = DecodeHints::default();
        let bytes = serde_json::to_vec(&empty).unwrap();
        // "{}" is 2 bytes. Anything <= 4 means every Option got skipped.
        assert!(bytes.len() <= 4, "empty hints encoded as {bytes:?}");
    }

    #[test]
    fn embedding_request_round_trip() {
        let r = EmbeddingRequest {
            id: "e1".into(),
            input: EmbeddingInput::Texts(vec!["hello".into(), "world".into()]),
            codec: Some("spine:codec/titans/v1@dim=256,dtype=f32".into()),
            trace: None,
        };
        json_round_trip(&r);
    }

    #[test]
    fn embedding_response_round_trip() {
        let r = EmbeddingResponse {
            id: "e1".into(),
            codec: "spine:codec/titans/v1@dim=4,dtype=f32".into(),
            embeddings: vec![EncodedFrame {
                codec: "spine:codec/titans/v1@dim=4,dtype=f32".into(),
                variant: None,
                data: 0u32.to_le_bytes().repeat(4),
                metadata: EncodedMetadata {
                    modality: Modality::Embedding,
                    shape: vec![4],
                    dtype: DType::F32,
                    original_len: Some(5),
                    source_hash: None,
                },
                trace: None,
            }],
            trace: None,
        };
        json_round_trip(&r);
    }

    /// A throwaway codec that turns "text" into its byte representation
    /// and a decoder that returns the bytes verbatim. Lets us exercise
    /// the NeuralCodec trait + registry without spinning up Titans.
    struct EchoCodec;
    impl NeuralCodec for EchoCodec {
        fn id(&self) -> &str {
            "spine:codec/echo/v1"
        }
        fn describe(&self) -> CodecDescriptor {
            CodecDescriptor {
                id: self.id().into(),
                description: "identity codec for tests".into(),
                direction: CodecDirection::Both,
                modality: Modality::Text,
                embedding_dim: None,
                vocab_size: None,
                dtype: DType::U8,
                semantic_embedding: None,
            }
        }
        fn encode(&self, input: &[u8]) -> Result<EncodedFrame, CodecError> {
            Ok(EncodedFrame {
                codec: self.id().into(),
                variant: None,
                data: input.to_vec(),
                metadata: EncodedMetadata {
                    modality: Modality::Text,
                    shape: vec![input.len() as u32],
                    dtype: DType::U8,
                    original_len: Some(input.len() as u64),
                    source_hash: None,
                },
                trace: None,
            })
        }
        fn decode(&self, frame: &EncodedFrame) -> Result<Vec<u8>, CodecError> {
            Ok(frame.data.clone())
        }
    }

    #[test]
    fn registry_round_trip_via_echo_codec() {
        let reg = CodecRegistry::new();
        reg.register(Arc::new(EchoCodec));
        assert_eq!(reg.ids(), vec!["spine:codec/echo/v1"]);

        let frame = reg.encode("spine:codec/echo/v1", b"hello").unwrap();
        assert_eq!(&frame.data, b"hello");
        let back = reg.decode(&frame).unwrap();
        assert_eq!(back, b"hello");
    }

    #[test]
    fn registry_unknown_codec_surfaces_typed_error() {
        let reg = CodecRegistry::new();
        let err = reg.encode("spine:codec/missing", b"x").unwrap_err();
        assert!(matches!(err, CodecError::UnknownCodec(_)));
    }

    #[test]
    fn registry_negotiation_picks_first_overlap() {
        let reg = CodecRegistry::new();
        reg.register(Arc::new(EchoCodec));
        let offered = vec![
            "openai:text-embedding-3-large".into(),
            "spine:codec/echo/v1".into(),
            "spine:codec/titans/v1".into(),
        ];
        assert_eq!(reg.pick_first(&offered).as_deref(), Some("spine:codec/echo/v1"));
        let no_match = reg.pick_first(&["unrelated".into()]);
        assert_eq!(no_match, None);
    }

    #[test]
    fn registry_advertisement_lists_registered_codecs() {
        let reg = CodecRegistry::new();
        reg.register(Arc::new(EchoCodec));
        let ad = reg.advertise("agent-a", "ad-1");
        assert_eq!(ad.id, "ad-1");
        assert_eq!(ad.agent_id, "agent-a");
        assert_eq!(ad.codecs.len(), 1);
        assert_eq!(ad.codecs[0].id, "spine:codec/echo/v1");
        json_round_trip(&ad);
    }

    /// REGRESSION GUARD — the stateless TitansLatentCodec must be
    /// idempotent: encoding the same input twice through the *same*
    /// codec (the shared-registry case) must yield byte-identical
    /// output. If this ever flips, request A's content is leaking into
    /// request B's embedding via the moving-target morph.
    #[test]
    fn titans_stateless_codec_does_not_leak_across_calls() {
        let codec = TitansLatentCodec::new(32);
        let a1 = codec.encode(b"secret-query-A").expect("encode");
        let b1 = codec.encode(b"innocent-query-B").expect("encode");
        let a2 = codec.encode(b"secret-query-A").expect("encode");
        let b2 = codec.encode(b"innocent-query-B").expect("encode");

        assert_eq!(
            a1.data, a2.data,
            "encoding `secret-query-A` was influenced by `innocent-query-B` running between calls — \
             state is leaking across requests"
        );
        assert_eq!(
            b1.data, b2.data,
            "encoding `innocent-query-B` differed between calls — moving-target morph is carrying \
             prior content into subsequent encodings"
        );
        assert_ne!(
            a1.data, b1.data,
            "different inputs should still produce different latents"
        );
    }

    /// Opt-in stateful codec: history MUST influence subsequent encodes
    /// (otherwise the "context-aware" mode does nothing).
    #[test]
    fn titans_stateful_codec_is_context_aware() {
        let codec = TitansLatentCodec::stateful(32, 32, 4, 0xABCD);
        let a1 = codec.encode(b"q").expect("encode");
        let _b = codec.encode(b"other").expect("encode");
        let a2 = codec.encode(b"q").expect("encode");
        // Same input, but `b` ran between — context shifted, embedding
        // must differ. (If this flips, the stateful constructor is
        // silently behaving like stateless.)
        assert_ne!(
            a1.data, a2.data,
            "stateful codec produced identical embeddings for the same input — \
             context-aware mode is not threading history through the morph"
        );
    }

    #[test]
    fn titans_codec_encodes_real_input() {
        let codec = TitansLatentCodec::new(32);
        let frame = codec.encode(b"the quick brown fox").expect("encode");
        // Titans encoder is fixed-width — shape[0] == embed_dim, bytes
        // == 4 × embed_dim for f32.
        assert_eq!(frame.metadata.dtype, DType::F32);
        assert_eq!(frame.metadata.modality, Modality::Embedding);
        assert!(frame.metadata.source_hash.is_some());
        assert_eq!(frame.metadata.shape, vec![32]);
        assert!(frame.declared_size_consistent());
        assert_eq!(frame.data.len(), 32 * 4);

        // The Titans pipeline is stateful — same fresh codec + same
        // seed + same input → same frame bytes.
        let codec_a = TitansLatentCodec::with_dims(32, 32, 4, 0xABCD);
        let codec_b = TitansLatentCodec::with_dims(32, 32, 4, 0xABCD);
        let a = codec_a.encode(b"hello").expect("encode a");
        let b = codec_b.encode(b"hello").expect("encode b");
        assert_eq!(a.data, b.data, "deterministic with fresh state + same seed");

        // Different input → different bytes (fresh codec each time).
        let codec_c = TitansLatentCodec::with_dims(32, 32, 4, 0xABCD);
        let c = codec_c.encode(b"completely different").expect("encode c");
        assert_ne!(a.data, c.data);

        // Decode is intentionally one-way for the projector codec.
        assert!(matches!(
            codec.decode(&frame),
            Err(CodecError::EncodeOnly(_))
        ));
    }
}
