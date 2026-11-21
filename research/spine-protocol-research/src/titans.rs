//! `TitansLatentCodec`: a [`NeuralCodec`] over `spine-neural`'s Titans
//! `NeuralLatentEncoder`. It moved here from `spine-protocol::agentic_codec`,
//! together with the codec trait, registry and frame types.

use crate::{
    CodecDescriptor, CodecDirection, CodecError, DType, EncodedFrame, EncodedMetadata, Modality,
    NeuralCodec,
};

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
/// wide [`crate::CodecRegistry`] and call from any number of concurrent
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
    /// [`crate::CodecRegistry`] that might serve more than one tenant; that
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

#[cfg(test)]
mod tests {
    use super::*;

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
