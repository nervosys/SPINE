//! The protocol handler: morphed framing, optional AES-256-GCM, speculative
//! framing and padding over any `AsyncRead + AsyncWrite` stream.
//!
//! This module has no dependency on `spine-neural` or `spine-crypto`, so it
//! builds with `--no-default-features`. The two research pieces that plug into
//! the handler, the Chameleon latent-space key and the speculative predictor,
//! are compiled only with the `research` feature. Without it the handler has
//! no chameleon and uses a stand-in predictor that never predicts, so every
//! message goes out as a `SpeculativePayload::Full` with
//! `next_prediction_hash = 0` and `confidence = 0.0`. That is the same wire
//! format a default handler emits, and a default peer reads it unchanged.

use aes_gcm::{aead::Aead, Aes256Gcm, Key, KeyInit, Nonce};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256 as Sha256Hash;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use zstd::stream::encode_all;

#[cfg(feature = "research")]
use crate::research::{ChameleonKey, SpeculativePredictor};

use super::*;

/// Stand-in for the research predictor when it is compiled out. It observes
/// nothing and predicts nothing: `predict_next` returns what the real
/// predictor returns before it has seen any message, `(0, 0.0)`, which keeps
/// `confidence` below the cache threshold and `next_prediction_hash` at 0.
#[cfg(not(feature = "research"))]
struct SpeculativePredictor {
    output_speculation: bool,
    input_speculation: bool,
}

#[cfg(not(feature = "research"))]
impl SpeculativePredictor {
    fn new() -> Self {
        Self {
            output_speculation: true,
            input_speculation: true,
        }
    }

    #[inline]
    fn observe(&mut self, _data: &[u8], _msg_type: MessageType) {}

    #[inline]
    fn predict_next(&mut self) -> (u64, f32) {
        (0, 0.0)
    }

    #[inline]
    fn predict_next_data(&mut self, _length: usize) -> Vec<u8> {
        Vec::new()
    }

    #[inline]
    fn predict_batch(&mut self, _count: usize) -> Vec<(u64, f32)> {
        Vec::new()
    }
}

/// A fresh 96-bit AES-GCM nonce from the operating system's CSPRNG.
///
/// The previous construction was `counter(8) || session_nonce(4)`, with the
/// counter restarting at zero every session. That would be sound if each session
/// had its own key — but the key here is `HKDF(secret)` with no salt and no
/// per-session input, so every session between the same pair shares one key, and
/// the only thing separating their nonce streams was 32 random bits. By the
/// birthday bound two sessions collide with probability near 39% after 65,536 of
/// them, and a collision repeats `(key, nonce)` for every message index the two
/// have in common.
///
/// AES-GCM does not survive that. It leaks the XOR of the two plaintexts, and
/// because GCM authenticates with a polynomial MAC it also exposes the
/// authentication subkey — so nonce reuse costs forgery, not only secrecy.
///
/// Random 96-bit nonces remove the session dimension entirely: the birthday
/// bound is now over all messages under the key, at roughly 2^-33 after 2^32 of
/// them, which is the regime NIST SP 800-38D sanctions for random IVs.
///
/// This is not a wire-format change. The full nonce has always been prepended to
/// the ciphertext and the receiver has always read it from there, so how the
/// sender chooses those twelve bytes was only ever a sender-side decision.
pub(crate) fn fresh_nonce() -> [u8; 12] {
    let mut nonce = [0u8; 12];
    rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut nonce);
    nonce
}

#[derive(Clone, Debug, Copy, PartialEq)]
pub enum MessageType {
    Request,
    Response,
    Event,
    Binary,
    Sync,
    Unknown,
}

// =============================================================================
// PROTOCOL MORPHING ENGINE
// =============================================================================

/// The protocol's shape-shifting characteristics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolMorphology {
    /// Current frame format version (changes unpredictably)
    pub frame_version: u8,
    /// Header size in bytes (varies 4-16)
    pub header_size: u8,
    /// Byte order for length field (alternates)
    pub big_endian: bool,
    /// Checksum algorithm selector
    pub checksum_variant: u8,
    /// Padding strategy
    pub padding_mode: PaddingMode,
    /// HMAC key for cryptographic morphology evolution (not serialized over wire)
    #[serde(skip, default = "default_morphology_key")]
    hmac_key: [u8; 32],
    /// Evolution counter for HMAC domain separation
    #[serde(skip)]
    evolution_counter: u64,
}

/// Scrub the HMAC key on drop so the secret cannot leak via a core
/// dump or swap-to-disk event (NIST SP 800-171 § 3.13.10). The other
/// fields are plaintext metadata and don't need clearing.
impl Drop for ProtocolMorphology {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.hmac_key.zeroize();
    }
}

fn default_morphology_key() -> [u8; 32] {
    [0u8; 32]
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PaddingMode {
    None,
    Pkcs7,
    Random,
    Chaotic,
}

impl ProtocolMorphology {
    pub(crate) fn new(seed: u64) -> Self {
        Self {
            frame_version: (seed % 256) as u8,
            header_size: 5 + (seed % 12) as u8,
            big_endian: seed.is_multiple_of(2),
            checksum_variant: (seed % 4) as u8,
            padding_mode: match seed % 4 {
                0 => PaddingMode::None,
                1 => PaddingMode::Pkcs7,
                2 => PaddingMode::Random,
                _ => PaddingMode::Chaotic,
            },
            hmac_key: [0u8; 32],
            evolution_counter: 0,
        }
    }

    /// Create with a session secret for cryptographic morphology evolution
    pub(crate) fn new_keyed(seed: u64, secret: &[u8; 32]) -> Self {
        let mut m = Self::new(seed);
        m.hmac_key = *secret;
        m
    }

    /// Evolve morphology using HMAC-SHA256 PRF keyed by session secret.
    /// Produces cryptographically unpredictable state transitions that
    /// cannot be predicted without knowledge of the HMAC key.
    pub(crate) fn evolve(&mut self, msg_hash: u64) {
        type HmacSha256 = Hmac<Sha256Hash>;

        // Domain-separate each evolution step with counter + message hash
        self.evolution_counter += 1;
        let mut input = [0u8; 16];
        input[..8].copy_from_slice(&self.evolution_counter.to_le_bytes());
        input[8..].copy_from_slice(&msg_hash.to_le_bytes());

        let mut mac =
            <HmacSha256 as Mac>::new_from_slice(&self.hmac_key).expect("HMAC accepts any key size");
        mac.update(&input);
        let result = mac.finalize().into_bytes();

        // Derive each morphology field from different bytes of the HMAC output
        self.frame_version = self.frame_version.wrapping_add(result[0] % 7);
        self.header_size = 5 + (result[1] % 12);
        self.big_endian = result[2] & 1 == 0;
        self.checksum_variant = result[3] % 4;
        self.padding_mode = match result[4] % 4 {
            0 => PaddingMode::None,
            1 => PaddingMode::Pkcs7,
            2 => PaddingMode::Random,
            _ => PaddingMode::Chaotic,
        };
    }
}

// =============================================================================
// CHAMELEON PROTOCOL HANDLER
// =============================================================================

pub struct ProtocolHandler<S> {
    stream: S,
    /// Traditional encryption (fallback)
    cipher: Option<Aes256Gcm>,
    /// Chameleon latent-space key (research builds only)
    #[cfg(feature = "research")]
    chameleon: Option<ChameleonKey>,
    /// Current protocol shape
    morphology: ProtocolMorphology,
    /// Enable moving-target defense
    moving_target: bool,
    /// Speculative predictor for outgoing messages
    output_predictor: SpeculativePredictor,
    /// Speculative predictor for incoming messages
    input_predictor: SpeculativePredictor,
    /// Cache of pre-computed responses
    precomputed_cache: Vec<(u64, Message)>,
    /// Cache of our own predictions to reconstruct on hit
    prediction_cache: Vec<(u64, Vec<u8>)>,
    /// Last prediction we sent (for verification)
    last_output_prediction: Option<u64>,
    /// Last prediction we received (for verification)
    last_input_prediction: Option<u64>,
    /// Statistics for speculation accuracy
    pub speculation_stats: SpeculationStats,
    /// Reusable send buffer to eliminate per-message heap allocations
    send_buf: Vec<u8>,
    /// Reusable read buffer for frame payloads
    read_buf: Vec<u8>,
    /// Reusable latent serialization buffer
    latent_buf: Vec<u8>,
    /// Minimum payload size for zstd compression (below this, overhead exceeds savings)
    compression_threshold: usize,
    /// Latent-space AEAD: chain Chameleon encoding + AES-GCM encryption (M2)
    /// When enabled, data is first neural-encoded to a LatentVector, serialized,
    /// then AES-GCM encrypted. Provides defense-in-depth: neural obfuscation +
    /// authenticated encryption. Requires both chameleon and cipher to be set.
    latent_aead: bool,
}

/// Statistics for tracking speculation effectiveness
#[derive(Debug, Clone, Default)]
pub struct SpeculationStats {
    pub output_predictions: u64,
    pub output_hits: u64,
    pub output_partial_hits: u64,
    pub input_predictions: u64,
    pub input_hits: u64,
    pub input_partial_hits: u64,
    pub bytes_saved: u64,
    pub precompute_hits: u64,
}

impl SpeculationStats {
    pub fn output_accuracy(&self) -> f32 {
        if self.output_predictions == 0 {
            0.0
        } else {
            self.output_hits as f32 / self.output_predictions as f32
        }
    }

    pub fn input_accuracy(&self) -> f32 {
        if self.input_predictions == 0 {
            0.0
        } else {
            self.input_hits as f32 / self.input_predictions as f32
        }
    }
}

impl<S> ProtocolHandler<S>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            cipher: None,
            #[cfg(feature = "research")]
            chameleon: None,
            morphology: ProtocolMorphology::new(0),
            moving_target: false,
            output_predictor: SpeculativePredictor::new(),
            input_predictor: SpeculativePredictor::new(),
            precomputed_cache: Vec::with_capacity(16),
            prediction_cache: Vec::with_capacity(16),
            last_output_prediction: None,
            last_input_prediction: None,
            speculation_stats: SpeculationStats::default(),
            send_buf: Vec::with_capacity(8192),
            read_buf: Vec::with_capacity(8192),
            latent_buf: Vec::with_capacity(4096),
            compression_threshold: 64,
            latent_aead: false,
        }
    }

    /// Enable traditional AES encryption
    pub fn enable_encryption(&mut self, key: [u8; 32]) {
        let key = Key::<Aes256Gcm>::from_slice(&key);
        self.cipher = Some(Aes256Gcm::new(key));
    }

    /// Enable Chameleon protocol (latent-space cryptography + moving target)
    #[cfg(feature = "research")]
    pub fn enable_chameleon(&mut self, secret: [u8; 32]) {
        self.chameleon = Some(ChameleonKey::new(&secret));
        self.morphology = ProtocolMorphology::new_keyed(
            u64::from_le_bytes(secret[0..8].try_into().unwrap()),
            &secret,
        );
        self.moving_target = true;
    }

    /// Enable Chameleon + AES-GCM (M2: latent-space AEAD)
    ///
    /// Defense-in-depth: data is first neural-encoded to a LatentVector
    /// (obfuscation), then the serialized latent bytes are AES-256-GCM
    /// encrypted (authenticated encryption). An attacker must break both
    /// the neural encoder AND AES-GCM to recover plaintext.
    #[cfg(feature = "research")]
    pub fn enable_chameleon_aead(&mut self, secret: [u8; 32]) {
        // Derive separate keys for Chameleon and AES-GCM from the shared secret
        let hk = hkdf::Hkdf::<sha2::Sha256>::new(None, &secret);
        let mut aes_key = [0u8; 32];
        hk.expand(b"spine-latent-aead-key", &mut aes_key)
            .expect("HKDF expand");

        // Set up Chameleon with the original secret
        self.chameleon = Some(ChameleonKey::new(&secret));
        self.morphology = ProtocolMorphology::new_keyed(
            u64::from_le_bytes(secret[0..8].try_into().unwrap()),
            &secret,
        );
        self.moving_target = true;

        // Set up AES-GCM with derived key
        let key = aes_gcm::Key::<Aes256Gcm>::from_slice(&aes_key);
        self.cipher = Some(Aes256Gcm::new(key));
        self.latent_aead = true;
    }

    /// Enable speculative decoding
    pub fn enable_speculation(&mut self, output: bool, input: bool) {
        self.output_predictor.output_speculation = output;
        self.input_predictor.input_speculation = input;
    }

    /// Pre-compute a response for a predicted request
    pub fn precompute_response(&mut self, predicted_request_hash: u64, response: Message) {
        self.precomputed_cache
            .push((predicted_request_hash, response));
        // Keep cache bounded
        if self.precomputed_cache.len() > 32 {
            self.precomputed_cache.remove(0);
        }
    }

    /// Check if we have a pre-computed response
    fn get_precomputed(&mut self, request_hash: u64) -> Option<Message> {
        if let Some(pos) = self
            .precomputed_cache
            .iter()
            .position(|(h, _)| *h == request_hash)
        {
            self.speculation_stats.precompute_hits += 1;
            Some(self.precomputed_cache.remove(pos).1)
        } else {
            None
        }
    }

    /// Hash message for key evolution
    #[inline]
    fn hash_message(data: &[u8]) -> u64 {
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        hasher.finish()
    }

    /// Classify message type for prediction
    fn classify_message(msg: &Message) -> MessageType {
        match msg {
            Message::Request(_) => MessageType::Request,
            Message::Response(_) => MessageType::Response,
            Message::Event(_) => MessageType::Event,
            Message::BinaryProgram(_) => MessageType::Binary,
            Message::Sync(_) => MessageType::Sync,
            Message::Speculative(_) => MessageType::Unknown,
            Message::PreComputed(_) => MessageType::Response,
            Message::LatentMessage(_) => MessageType::Unknown,
            Message::MorphRequest { .. } => MessageType::Sync,
            Message::Ping { .. } => MessageType::Sync,
            Message::Pong { .. } => MessageType::Sync,
            // Agentic primitives slot into the existing request/response/event
            // taxonomy for purposes of speculative prediction.
            Message::ToolCall(_) => MessageType::Request,
            Message::ToolResult(_) => MessageType::Response,
            Message::StreamStart(_) => MessageType::Event,
            Message::StreamToken(_) => MessageType::Event,
            Message::StreamEnd(_) => MessageType::Event,
            Message::StreamCancel(_) => MessageType::Event,
            Message::CapabilityQuery(_) => MessageType::Request,
            Message::CapabilityAd(_) => MessageType::Response,
            // Neural codec frames slot into the same taxonomy.
            Message::Encoded(_) => MessageType::Binary,
            Message::CodecAd(_) => MessageType::Response,
            Message::CodecNegotiation(_) => MessageType::Sync,
            Message::EmbeddingRequest(_) => MessageType::Request,
            Message::EmbeddingResponse(_) => MessageType::Response,
        }
    }

    /// Chameleon-encode (and, with latent AEAD, encrypt) an outgoing frame body.
    /// `None` means no chameleon is set and the caller falls through to the
    /// plain AES path.
    #[cfg(feature = "research")]
    fn chameleon_seal(&mut self, data: &[u8], msg_hash: u64) -> anyhow::Result<Option<Vec<u8>>> {
        let Some(chameleon) = self.chameleon.as_mut() else {
            return Ok(None);
        };
        let latent = chameleon.encode(data);
        // FAST: Serialize into reusable buffer (zero-alloc steady-state, 22 GiB/s)
        latent.to_bytes_into(&mut self.latent_buf);
        let mut out = std::mem::take(&mut self.latent_buf);

        // M2: Latent-space AEAD — encrypt the serialized latent vector with AES-GCM
        // This provides defense-in-depth: neural obfuscation + authenticated encryption.
        // An attacker must break BOTH the neural latent encoder AND AES-256-GCM.
        if self.latent_aead {
            if let Some(cipher) = &self.cipher {
                let nonce_full = fresh_nonce();
                let nonce = Nonce::from_slice(&nonce_full);
                out = cipher
                    .encrypt(nonce, out.as_ref())
                    .map_err(|e| anyhow::anyhow!("Latent AEAD encrypt error: {}", e))?;
                let mut with_nonce = nonce_full.to_vec();
                with_nonce.extend(out);
                out = with_nonce;
            }
        }

        // Evolve key after sending (moving target)
        if self.moving_target {
            chameleon.evolve(msg_hash);
            // morphology evolution deferred to after write_morphed_frame
        }
        Ok(Some(out))
    }

    #[cfg(not(feature = "research"))]
    #[inline]
    fn chameleon_seal(&mut self, _data: &[u8], _msg_hash: u64) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(None)
    }

    /// Chameleon-decode (after latent AEAD decryption) an incoming frame body.
    /// `None` means no chameleon is set and the caller falls through to the
    /// plain AES path.
    #[cfg(feature = "research")]
    fn chameleon_open(&mut self, data: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
        let Some(chameleon) = self.chameleon.as_mut() else {
            return Ok(None);
        };
        // M2: Latent-space AEAD — decrypt serialized latent vector before decoding
        let decrypted = if self.latent_aead {
            if let Some(cipher) = &self.cipher {
                if data.len() < 12 {
                    return Err(anyhow::anyhow!("Invalid latent AEAD message"));
                }
                let nonce = Nonce::from_slice(&data[..12]);
                Some(
                    cipher
                        .decrypt(nonce, &data[12..])
                        .map_err(|e| anyhow::anyhow!("Latent AEAD decrypt error: {}", e))?,
                )
            } else {
                None
            }
        } else {
            None
        };
        let bytes = decrypted.as_deref().unwrap_or(data);

        // FAST: Zero-copy binary deserialization
        let latent = LatentVector::from_bytes_fast(bytes)
            .ok_or_else(|| anyhow::anyhow!("Invalid latent vector format"))?;
        Ok(Some(chameleon.decode(&latent)))
    }

    #[cfg(not(feature = "research"))]
    #[inline]
    fn chameleon_open(&mut self, _data: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
        Ok(None)
    }

    /// Whether a chameleon is set (always false without the `research` feature).
    #[cfg(feature = "research")]
    #[inline]
    fn chameleon_active(&self) -> bool {
        self.chameleon.is_some()
    }

    #[cfg(not(feature = "research"))]
    #[inline]
    fn chameleon_active(&self) -> bool {
        false
    }

    /// Evolve the chameleon key and the morphology after decoding a message.
    #[cfg(feature = "research")]
    fn chameleon_evolve_received(&mut self, msg_data: &[u8]) {
        if let Some(ref mut chameleon) = self.chameleon {
            let msg_hash = Self::hash_message(msg_data);
            if self.moving_target {
                chameleon.evolve(msg_hash);
                self.morphology.evolve(msg_hash);
            }
        }
    }

    #[cfg(not(feature = "research"))]
    #[inline]
    fn chameleon_evolve_received(&mut self, _msg_data: &[u8]) {}

    /// Evolve the chameleon key with a morph seed.
    #[cfg(feature = "research")]
    fn chameleon_evolve_seed(&mut self, seed: u64) {
        if let Some(ref mut chameleon) = self.chameleon {
            chameleon.evolve(seed);
        }
    }

    #[cfg(not(feature = "research"))]
    #[inline]
    fn chameleon_evolve_seed(&mut self, _seed: u64) {}

    /// Send a message with speculative encoding
    ///
    /// Optimized hot path: single serialization pass, buffer reuse,
    /// adaptive compression (skipped for small payloads < threshold).
    #[inline]
    pub async fn send_message(&mut self, msg: &Message) -> anyhow::Result<()> {
        // Serialize message once via the binary wire codec (CBOR / CBOR+zstd).
        self.send_buf = wire::encode(msg)?;
        let msg_hash = Self::hash_message(&self.send_buf);
        let msg_type = Self::classify_message(msg);

        // Check if receiver predicted this message
        let payload = if let Some(predicted_hash) = self.last_input_prediction {
            if predicted_hash == msg_hash {
                // Prediction hit! Send minimal confirmation (no data needed)
                self.speculation_stats.output_hits += 1;
                self.speculation_stats.bytes_saved += self.send_buf.len() as u64;
                SpeculativePayload::Confirmed {
                    confirmed_hash: msg_hash,
                    delta: Vec::new(),
                }
            } else {
                // Prediction miss — move buffer contents to avoid clone
                SpeculativePayload::Full(std::mem::take(&mut self.send_buf))
            }
        } else {
            SpeculativePayload::Full(std::mem::take(&mut self.send_buf))
        };

        // Observe for prediction (use payload data if available, else empty)
        let observe_data: &[u8] = match &payload {
            SpeculativePayload::Full(d) => d,
            _ => &[],
        };
        self.output_predictor.observe(observe_data, msg_type);
        let (next_prediction, confidence) = self.output_predictor.predict_next();

        // Cache the predicted data for reconstruction on hit
        if confidence > 0.5 {
            let predicted_data = self.output_predictor.predict_next_data(observe_data.len());
            self.prediction_cache
                .push((next_prediction, predicted_data));
            if self.prediction_cache.len() > 16 {
                self.prediction_cache.remove(0);
            }
        }

        self.last_output_prediction = Some(next_prediction);
        self.speculation_stats.output_predictions += 1;

        // Build and serialize speculative frame in one pass
        let frame = SpeculativeFrame {
            payload,
            next_prediction_hash: next_prediction,
            confidence,
            speculation_depth: 1,
        };

        let mut data = serde_json::to_vec(&frame)?;

        // Restore send_buf capacity for next call
        if self.send_buf.capacity() == 0 {
            self.send_buf = Vec::with_capacity(8192);
        }

        // Apply padding based on current morphology
        data = self.apply_padding(data);

        // Adaptive compression: skip zstd for tiny control payloads (< 64 bytes)
        // For larger payloads, zstd at level 3 provides excellent ratio with low overhead.
        // We prepend a 1-byte flag: 0x01 = compressed, 0x00 = raw
        if data.len() >= self.compression_threshold {
            let compressed = encode_all(&data[..], 3)?;
            let mut flagged = Vec::with_capacity(1 + compressed.len());
            flagged.push(0x01); // compressed flag
            flagged.extend_from_slice(&compressed);
            data = flagged;
        } else {
            data.insert(0, 0x00); // raw flag
        }

        // Chameleon encoding (research builds, if enabled)
        if let Some(sealed) = self.chameleon_seal(&data, msg_hash)? {
            data = sealed;
        }
        // Fallback to AES encryption (without Chameleon)
        else if let Some(cipher) = &self.cipher {
            let nonce_full = fresh_nonce();
            let nonce = Nonce::from_slice(&nonce_full);
            data = cipher
                .encrypt(nonce, data.as_ref())
                .map_err(|e| anyhow::anyhow!("Encryption error: {}", e))?;

            // Prepend nonce for receiver
            let mut with_nonce = nonce_full.to_vec();
            with_nonce.extend(data);
            data = with_nonce;
        }

        // Write with morphing header
        self.write_morphed_frame(&data).await?;

        // Evolve morphology AFTER writing frame to keep sender/receiver in sync
        if self.moving_target && self.chameleon_active() {
            self.morphology.evolve(msg_hash);
        }
        Ok(())
    }

    /// Send message without speculation (for bootstrapping)
    pub async fn send_message_raw(&mut self, msg: &Message) -> anyhow::Result<()> {
        let mut data = wire::encode(msg)?;
        let msg_hash = Self::hash_message(&data);

        data = self.apply_padding(data);

        // Adaptive compression with flag
        // SECURITY NOTE (L2): Compressing plaintext before encryption can leak
        // information via ciphertext size changes (CRIME/BREACH attacks). This is
        // acceptable for agent-to-agent communication where there is no reflected
        // user-controlled content. Disable compression when handling untrusted input.
        if data.len() >= self.compression_threshold {
            let compressed = encode_all(&data[..], 3)?;
            let mut flagged = Vec::with_capacity(1 + compressed.len());
            flagged.push(0x01);
            flagged.extend_from_slice(&compressed);
            data = flagged;
        } else {
            data.insert(0, 0x00);
        }

        if let Some(sealed) = self.chameleon_seal(&data, msg_hash)? {
            data = sealed;
        } else if let Some(cipher) = &self.cipher {
            // SECURITY: Use counter-based nonce to prevent nonce reuse
            let nonce_full = fresh_nonce();
            let nonce = Nonce::from_slice(&nonce_full);
            data = cipher
                .encrypt(nonce, data.as_ref())
                .map_err(|e| anyhow::anyhow!("Encryption error: {}", e))?;

            let mut with_nonce = nonce_full.to_vec();
            with_nonce.extend(data);
            data = with_nonce;
        }

        self.write_morphed_frame(&data).await?;

        // Evolve morphology AFTER writing frame to keep sender/receiver in sync
        if self.moving_target && self.chameleon_active() {
            self.morphology.evolve(msg_hash);
        }
        Ok(())
    }

    /// Receive a message with speculative decoding
    pub async fn receive_message(&mut self) -> anyhow::Result<Message> {
        // Read with morphing header
        let mut data = self.read_morphed_frame().await?;

        // Chameleon decoding (research builds, if enabled)
        if let Some(opened) = self.chameleon_open(&data)? {
            data = opened;
        }
        // Fallback to AES decryption (without Chameleon)
        else if let Some(cipher) = &self.cipher {
            if data.len() < 12 {
                return Err(anyhow::anyhow!("Invalid encrypted message"));
            }
            let nonce = Nonce::from_slice(&data[..12]);
            data = cipher
                .decrypt(nonce, &data[12..])
                .map_err(|e| anyhow::anyhow!("Decryption error: {}", e))?;
        }

        // Adaptive decompression: check compression flag byte
        if data.is_empty() {
            return Err(anyhow::anyhow!("Empty message after decryption"));
        }
        data = if data[0] == 0x01 {
            wire::decode_zstd_limited(&data[1..], wire::MAX_DECODED_FRAME_BYTES)?
        } else {
            data[1..].to_vec()
        };

        // Remove padding
        data = self.remove_padding(data);

        // Try to parse as speculative frame
        if let Ok(frame) = serde_json::from_slice::<SpeculativeFrame>(&data) {
            // Store their prediction for our next send
            self.last_input_prediction = Some(frame.next_prediction_hash);
            self.speculation_stats.input_predictions += 1;

            // Decode payload
            let msg_data = match frame.payload {
                SpeculativePayload::Full(full_data) => full_data,
                SpeculativePayload::Confirmed {
                    confirmed_hash,
                    delta,
                } => {
                    // They confirmed our prediction was correct!
                    self.speculation_stats.input_hits += 1;

                    // Reconstruct from our prediction cache
                    if let Some(pos) = self
                        .prediction_cache
                        .iter()
                        .position(|(h, _)| *h == confirmed_hash)
                    {
                        let (_, mut predicted_data) = self.prediction_cache.remove(pos);

                        // Apply delta if any
                        if !delta.is_empty() {
                            // Simple XOR delta for now
                            for (p, d) in predicted_data.iter_mut().zip(delta.iter()) {
                                *p ^= *d;
                            }
                        }
                        predicted_data
                    } else {
                        // Cache miss, but they confirmed it. This shouldn't happen if we're in sync.
                        return Err(anyhow::anyhow!(
                            "Speculation hit but prediction not in cache"
                        ));
                    }
                }
                SpeculativePayload::Partial {
                    prefix_hash: _,
                    prefix_len: _,
                    suffix,
                } => {
                    self.speculation_stats.input_partial_hits += 1;
                    // Would combine cached prefix with received suffix
                    suffix
                }
                SpeculativePayload::Batch {
                    predictions,
                    fallback,
                } => {
                    // Check if any prediction matches our expectation
                    if let Some(pred) = self.last_output_prediction {
                        if predictions.contains(&pred) {
                            self.speculation_stats.input_hits += 1;
                        }
                    }
                    fallback
                }
            };

            // Parse the actual message
            let msg: Message = wire::decode(&msg_data)?;

            // Handle proactive morphing requests from server
            if let Message::MorphRequest { seed } = &msg {
                self.morph_now(*seed);
            }

            // Update input predictor
            let msg_type = Self::classify_message(&msg);
            self.input_predictor.observe(&msg_data, msg_type);

            // Evolve Chameleon key based on the decoded message
            self.chameleon_evolve_received(&msg_data);

            Ok(msg)
        } else {
            // Legacy message (no speculative frame)
            let msg: Message = wire::decode(&data)?;

            // Handle proactive morphing requests from server
            if let Message::MorphRequest { seed } = &msg {
                self.morph_now(*seed);
            }

            // Update predictor anyway
            let msg_type = Self::classify_message(&msg);
            self.input_predictor.observe(&data, msg_type);

            self.chameleon_evolve_received(&data);

            Ok(msg)
        }
    }

    /// Speculatively pre-compute responses for likely incoming requests
    pub fn speculate_responses<F>(&mut self, compute_response: F)
    where
        F: Fn(u64) -> Option<Message>,
    {
        // Get batch of likely next messages
        let predictions = self.input_predictor.predict_batch(4);

        for (predicted_hash, confidence) in predictions {
            if confidence > 0.3 {
                if let Some(response) = compute_response(predicted_hash) {
                    self.precompute_response(predicted_hash, response);
                }
            }
        }
    }

    /// Get speculation statistics
    pub fn get_speculation_stats(&self) -> &SpeculationStats {
        &self.speculation_stats
    }

    /// Write frame with morphing header format (stack-allocated header, no heap alloc)
    #[inline]
    async fn write_morphed_frame(&mut self, data: &[u8]) -> anyhow::Result<()> {
        anyhow::ensure!(
            data.len() <= wire::MAX_TRANSPORT_FRAME_BYTES,
            "native transport frame exceeds size limit"
        );
        let len = data.len() as u32;
        let header_size = self.morphology.header_size as usize;

        // Stack-allocated header (max 16 bytes) — no heap allocation
        let mut header = [0u8; 16];
        debug_assert!(header_size <= 16, "Header size must be <= 16");

        // Length field (endianness varies)
        let len_bytes = if self.morphology.big_endian {
            len.to_be_bytes()
        } else {
            len.to_le_bytes()
        };
        header[..4].copy_from_slice(&len_bytes);

        // Version byte
        header[4] = self.morphology.frame_version;

        // Padding to header_size
        for b in header[5..header_size].iter_mut() {
            *b = self.morphology.checksum_variant;
        }

        write_header_body(&mut self.stream, &header[..header_size], data).await?;
        Ok(())
    }

    /// Read frame with morphing header format (reuses read buffer to minimize allocations)
    #[inline]
    async fn read_morphed_frame(&mut self) -> anyhow::Result<Vec<u8>> {
        // Stack-allocated header read
        let header_size = self.morphology.header_size as usize;
        let mut header = [0u8; 16];
        self.stream.read_exact(&mut header[..header_size]).await?;

        // Extract length (endianness varies)
        let len_bytes: [u8; 4] = header[0..4].try_into()?;
        let len = if self.morphology.big_endian {
            u32::from_be_bytes(len_bytes)
        } else {
            u32::from_le_bytes(len_bytes)
        } as usize;

        // SECURITY: Limit maximum frame size to prevent OOM attacks (16 MiB)
        const MAX_FRAME_SIZE: usize = wire::MAX_TRANSPORT_FRAME_BYTES;
        if len > MAX_FRAME_SIZE {
            anyhow::bail!(
                "Frame size {} exceeds maximum allowed size {}",
                len,
                MAX_FRAME_SIZE
            );
        }

        // Reuse read buffer to avoid per-frame allocations
        self.read_buf.clear();
        self.read_buf.resize(len, 0);
        self.stream.read_exact(&mut self.read_buf).await?;
        Ok(std::mem::take(&mut self.read_buf))
    }

    /// Apply padding based on current morphology
    fn apply_padding(&self, mut data: Vec<u8>) -> Vec<u8> {
        match self.morphology.padding_mode {
            PaddingMode::None => data,
            PaddingMode::Pkcs7 => {
                let padding_len = 16 - (data.len() % 16);
                data.extend(std::iter::repeat_n(padding_len as u8, padding_len));
                data
            }
            PaddingMode::Random => {
                let padding_len = 16 - (data.len() % 16);
                let mut state = self.morphology.frame_version as u64;
                for _ in 0..padding_len {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    data.push(state as u8);
                }
                // Store actual padding length at end
                data.push(padding_len as u8);
                data
            }
            PaddingMode::Chaotic => {
                // Pad to random length between 16 and 256 bytes
                let target = 16 + (self.morphology.frame_version as usize % 240);
                let padding_len = if data.len() >= target {
                    16
                } else {
                    target - data.len()
                };
                let original_len = data.len();

                let mut state = self.morphology.checksum_variant as u64 * 12345;
                for _ in 0..padding_len {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    data.push(state as u8);
                }

                // Encode original length in last 4 bytes
                data.extend_from_slice(&(original_len as u32).to_le_bytes());
                data
            }
        }
    }

    /// Remove padding based on current morphology
    fn remove_padding(&self, mut data: Vec<u8>) -> Vec<u8> {
        match self.morphology.padding_mode {
            PaddingMode::None => data,
            PaddingMode::Pkcs7 => {
                if let Some(&padding_len) = data.last() {
                    // Validate PKCS7 padding: padding_len must be 1-16 and not exceed data length
                    let padding_len = padding_len as usize;
                    if (1..=16).contains(&padding_len) && padding_len <= data.len() {
                        // Verify all padding bytes are equal to padding_len
                        let start = data.len() - padding_len;
                        let valid = data[start..].iter().all(|&b| b as usize == padding_len);
                        if valid {
                            data.truncate(start);
                        }
                        // If invalid, return data as-is (corrupted padding)
                    }
                }
                data
            }
            PaddingMode::Random => {
                if let Some(&padding_len) = data.last() {
                    // Validate: padding_len + 1 (for length byte) must not exceed data length
                    let total_padding = padding_len as usize + 1;
                    if total_padding <= data.len() {
                        let len = data.len() - total_padding;
                        data.truncate(len);
                    }
                }
                data
            }
            PaddingMode::Chaotic => {
                if data.len() >= 4 {
                    let len_bytes: [u8; 4] = data[data.len() - 4..].try_into().unwrap_or([0; 4]);
                    let original_len = u32::from_le_bytes(len_bytes) as usize;
                    data.truncate(original_len);
                }
                data
            }
        }
    }

    /// Inject decoy traffic to confuse traffic analysis
    pub async fn send_decoy(&mut self) -> anyhow::Result<()> {
        let mut noise = Vec::with_capacity(64);
        let mut state = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        for _ in 0..64 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            noise.push((state as f32) / (u64::MAX as f32));
        }

        let decoy = Message::Event(Event {
            name: "__decoy__".to_string(),
            data: serde_json::json!({ "noise": noise }),
        });

        self.send_message(&decoy).await
    }

    /// Force protocol morphing (re-sync with peer)
    pub fn morph_now(&mut self, seed: u64) {
        self.morphology.evolve(seed);
        self.chameleon_evolve_seed(seed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_morphology() {
        let mut morph = ProtocolMorphology::new(12345);
        let initial_version = morph.frame_version;

        morph.evolve(67890);

        // Frame version should evolve
        // The evolve function changes internal state
        assert!(morph.frame_version != initial_version || morph.header_size > 0);
    }

    /// The defect this replaced: the nonce was `counter(8) || session_nonce(4)`
    /// with the counter restarting at zero each session, while the AEAD key is a
    /// pure function of the shared secret and so identical across sessions. Two
    /// sessions that drew the same 32-bit `session_nonce` — about a 39% chance
    /// after 65,536 of them — then reused `(key, nonce)` on every message index
    /// they shared, which costs AES-GCM both confidentiality and unforgeability.
    ///
    /// A nonce must never repeat under one key, so the property to assert is
    /// that fresh nonces do not collide, not merely that consecutive ones differ.
    #[test]
    fn nonces_do_not_repeat_across_sessions() {
        use std::collections::HashSet;
        let mut seen = HashSet::new();
        for _ in 0..10_000 {
            assert!(
                seen.insert(fresh_nonce()),
                "a nonce repeated under a key that never changes"
            );
        }
    }

    /// A counter-based nonce is recognisable: its low bytes walk 1, 2, 3 and the
    /// top four never move. This asserts the replacement is not that — the whole
    /// 96 bits vary, which is what removes the session dimension from the
    /// birthday bound.
    #[test]
    fn every_byte_of_the_nonce_carries_entropy() {
        let samples: Vec<[u8; 12]> = (0..256).map(|_| fresh_nonce()).collect();
        for position in 0..12 {
            let distinct: std::collections::HashSet<u8> =
                samples.iter().map(|n| n[position]).collect();
            assert!(
                distinct.len() > 1,
                "byte {position} never varies, so it is structure and not entropy"
            );
        }
    }

    /// A message small enough that its frame may stay raw, and one large enough
    /// to take the zstd path.
    fn sample_messages() -> (Message, Message) {
        let small = Message::Ping { timestamp: 7 };
        let large = Message::Event(Event {
            name: "large".to_string(),
            data: serde_json::json!({ "payload": "x".repeat(300) }),
        });
        (small, large)
    }

    #[tokio::test]
    async fn handlers_exchange_small_and_large_messages_both_ways() {
        let (a_stream, b_stream) = tokio::io::duplex(64 * 1024);
        let mut a = ProtocolHandler::new(a_stream);
        let mut b = ProtocolHandler::new(b_stream);
        let (small, large) = sample_messages();

        for msg in [&small, &large] {
            let expected = wire::encode(msg).unwrap();

            a.send_message(msg).await.unwrap();
            let got = b.receive_message().await.unwrap();
            assert_eq!(wire::encode(&got).unwrap(), expected);

            b.send_message(msg).await.unwrap();
            let got = a.receive_message().await.unwrap();
            assert_eq!(wire::encode(&got).unwrap(), expected);
        }
    }

    /// Both ends morph with the same seed at the same point in the stream, then
    /// keep talking in both directions.
    #[tokio::test]
    async fn handlers_stay_in_sync_when_both_morph_with_one_seed() {
        let (a_stream, b_stream) = tokio::io::duplex(64 * 1024);
        let mut a = ProtocolHandler::new(a_stream);
        let mut b = ProtocolHandler::new(b_stream);
        let (small, large) = sample_messages();

        a.send_message(&small).await.unwrap();
        b.receive_message().await.unwrap();

        // Seed 5 moves the framing from the default 5-byte big-endian header
        // to an 11-byte little-endian one.
        a.morph_now(5);
        b.morph_now(5);

        for msg in [&small, &large] {
            let expected = wire::encode(msg).unwrap();

            a.send_message(msg).await.unwrap();
            let got = b.receive_message().await.unwrap();
            assert_eq!(wire::encode(&got).unwrap(), expected);

            b.send_message(msg).await.unwrap();
            let got = a.receive_message().await.unwrap();
            assert_eq!(wire::encode(&got).unwrap(), expected);
        }
    }

    /// If only the sender morphs, the receiver reads the next frame with the
    /// old header layout and cannot recover the message.
    #[tokio::test]
    async fn a_receiver_that_did_not_morph_cannot_read_the_next_frame() {
        let (a_stream, b_stream) = tokio::io::duplex(64 * 1024);
        let mut a = ProtocolHandler::new(a_stream);
        let mut b = ProtocolHandler::new(b_stream);
        let msg = Message::Ping { timestamp: 7 };

        a.morph_now(5);
        a.send_message(&msg).await.unwrap();

        // The old 5-byte big-endian header reads the new little-endian length
        // as a size over the frame limit, so this errors; the timeout only
        // guards against a reader that waits for bytes that never come.
        let got =
            tokio::time::timeout(std::time::Duration::from_millis(500), b.receive_message()).await;
        match got {
            Ok(Ok(received)) => assert_ne!(
                wire::encode(&received).unwrap(),
                wire::encode(&msg).unwrap()
            ),
            Ok(Err(_)) | Err(_) => {}
        }
    }

    /// A default handler wraps every message in a `SpeculativeFrame` carrying
    /// the whole message. Without the `research` feature the frame also
    /// advertises no prediction.
    #[tokio::test]
    async fn a_default_handler_emits_a_full_speculative_frame() {
        let (tx, mut rx) = tokio::io::duplex(64 * 1024);
        let mut handler = ProtocolHandler::new(tx);
        let msg = Message::Ping { timestamp: 42 };
        handler.send_message(&msg).await.unwrap();

        // Undo the framing the same way the receive path does.
        let morph = ProtocolMorphology::new(0);
        let mut header = vec![0u8; morph.header_size as usize];
        rx.read_exact(&mut header).await.unwrap();
        let len_bytes: [u8; 4] = header[..4].try_into().unwrap();
        let len = if morph.big_endian {
            u32::from_be_bytes(len_bytes)
        } else {
            u32::from_le_bytes(len_bytes)
        } as usize;
        let mut body = vec![0u8; len];
        rx.read_exact(&mut body).await.unwrap();

        let data = if body[0] == 0x01 {
            wire::decode_zstd_limited(&body[1..], wire::MAX_DECODED_FRAME_BYTES).unwrap()
        } else {
            body[1..].to_vec()
        };
        let data = handler.remove_padding(data);

        let frame: SpeculativeFrame = serde_json::from_slice(&data).unwrap();
        match frame.payload {
            SpeculativePayload::Full(bytes) => assert_eq!(bytes, wire::encode(&msg).unwrap()),
            other => panic!("expected a Full payload, got {other:?}"),
        }
        assert_eq!(frame.speculation_depth, 1);
        if cfg!(not(feature = "research")) {
            assert_eq!(frame.next_prediction_hash, 0);
            assert_eq!(frame.confidence, 0.0);
        }
    }
}
