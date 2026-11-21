//! Research-only protocol machinery, compiled when the `research` feature is on.
//!
//! Everything here is experimental: the Chameleon latent-space key,
//! `SpineConnection`, and the protocol evolution framework. It is the only
//! part of the crate that needs `spine-neural` and `spine-crypto`. The wire
//! types, the codec and `ProtocolHandler` (which plugs the chameleon in when
//! this feature is on) do not depend on it, so `--no-default-features` builds
//! without either crate. With the feature on, every item below is re-exported
//! from the crate root, so public paths are unchanged.

use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};
use spine_crypto::{LatticeParams, QuantumSpeculativeProtocol, TransformerConfig};
use spine_neural::{MirasNeuralEncoder, MirasVariant, NeuralEncoderConfig, NeuralLatentEncoder};
use std::collections::hash_map::DefaultHasher;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use zstd::stream::encode_all;

use super::*;

mod evolution;
pub use evolution::*;

/// Derive the Chameleon layer's seed from a shared secret.
///
/// This replaced `DefaultHasher`, which was wrong for two independent reasons.
///
/// The cryptographic one: `DefaultHasher::new()` is SipHash keyed with zeros, a
/// hash designed for HashMap collision resistance rather than for deriving key
/// material. It offers no guarantee that its 64-bit output is a uniform,
/// secret-dependent value, which is exactly the guarantee a key derivation
/// needs. HKDF-SHA-256 does, and the info string separates this use from the
/// AEAD key derived from the same secret.
///
/// The correctness one, which is arguably worse: the algorithm behind
/// `DefaultHasher` is explicitly unspecified by the standard library and may
/// change between Rust releases. Two peers built with different compilers could
/// therefore derive different seeds from the same shared secret and fail to
/// understand each other, with nothing in the protocol to say why.
///
/// The output is still 64 bits, which is a ceiling this function cannot lift:
/// `NeuralLatentEncoder` and `TransformerConfig` are seeded by a `u64`. Raising
/// it means widening their keying, which is a change to those crates rather than
/// to this derivation. See `SECURITY_AUDIT.md`.
fn chameleon_seed(secret: &[u8; 32]) -> u64 {
    let hk = hkdf::Hkdf::<sha2::Sha256>::new(None, secret);
    let mut bytes = [0u8; 8];
    hk.expand(b"spine-chameleon-seed", &mut bytes)
        .expect("HKDF expand into 8 bytes cannot fail");
    u64::from_le_bytes(bytes)
}

/// Dynamic transformation matrix that evolves with each message.
/// This is the "key" in our latent cryptography system.
#[derive(Clone)]
pub struct ChameleonKey {
    /// Neural latent encoder (legacy)
    encoder: NeuralLatentEncoder,
    /// MIRAS-adaptive encoder for enhanced continual learning
    miras_encoder: Option<MirasNeuralEncoder>,
    /// Quantum-resistant key evolution
    quantum: QuantumSpeculativeProtocol,
    /// Dimension of the latent space
    dimension: usize,
    /// Message counter for key evolution
    epoch: u64,
    /// Anomaly history for adaptive MIRAS selection
    anomaly_scores: VecDeque<f32>,
    /// Current MIRAS variant in use
    active_variant: MirasVariant,
    /// Threshold for switching to YAAD (outlier-robust)
    anomaly_threshold: f32,
}

impl ChameleonKey {
    /// Create a new Chameleon key from a shared secret
    pub fn new(secret: &[u8; 32]) -> Self {
        let dimension = 128;
        let config = TransformerConfig {
            embed_dim: 64,
            num_heads: 4,
            num_layers: 4,
            ff_dim: 128,
            max_seq_len: 128,
            memory_size: 32, // Titans persistent memory tokens
            seed: 42,
        };
        let params = LatticeParams {
            n: 512,
            q: 12289,
            p: 12289,
            sigma: 3.2,
        };
        let seed = chameleon_seed(secret);

        Self {
            // attention_heads (8) must divide latent_dim (128): 128/8=16 ✓
            // Keyed from the whole secret: the encoder weights are the key here,
            // so the u64 seed below is only a per-instance diversifier for the
            // machinery that still takes one.
            encoder: NeuralLatentEncoder::new_keyed(dimension, 128, &[256, 128], 8, secret),
            miras_encoder: None,
            quantum: QuantumSpeculativeProtocol::new(config, params, seed),
            dimension,
            epoch: 0,
            anomaly_scores: VecDeque::with_capacity(100),
            active_variant: MirasVariant::Titans,
            anomaly_threshold: 5.0,
        }
    }

    /// Create with MIRAS-adaptive encoding for continual learning
    pub fn new_with_miras(secret: &[u8; 32], variant: MirasVariant) -> Self {
        let dimension = 128;
        let config = TransformerConfig {
            embed_dim: 64,
            num_heads: 4,
            num_layers: 4,
            ff_dim: 128,
            max_seq_len: 128,
            memory_size: 32,
            seed: 42,
        };
        let params = LatticeParams {
            n: 512,
            q: 12289,
            p: 12289,
            sigma: 3.2,
        };
        let seed = chameleon_seed(secret);

        let miras_config = NeuralEncoderConfig {
            input_dim: 256,
            latent_dim: dimension, // 128
            hidden_dims: vec![256, 128],
            attention_heads: 8, // 128/8=16 ✓
            seed,
            miras_variant: variant,
            memory_tokens: 16,
        };

        Self {
            // attention_heads (8) must divide latent_dim (128): 128/8=16 ✓
            // Keyed from the whole secret: the encoder weights are the key here,
            // so the u64 seed below is only a per-instance diversifier for the
            // machinery that still takes one.
            encoder: NeuralLatentEncoder::new_keyed(dimension, 128, &[256, 128], 8, secret),
            miras_encoder: Some(miras_config.build_miras()),
            quantum: QuantumSpeculativeProtocol::new(config, params, seed),
            dimension,
            epoch: 0,
            anomaly_scores: VecDeque::with_capacity(100),
            active_variant: variant,
            anomaly_threshold: 5.0,
        }
    }

    /// Encode data into the latent space
    pub fn encode(&mut self, data: &[u8]) -> LatentVector {
        let components = if let Some(ref mut miras) = self.miras_encoder {
            // Use MIRAS encoder with adaptive memory
            let encoded = miras.encode(data);

            // Track surprise for anomaly detection
            let surprise = miras.get_surprise();
            self.anomaly_scores.push_back(surprise);
            if self.anomaly_scores.len() > 100 {
                self.anomaly_scores.pop_front();
            }

            // Adaptive variant switching based on anomaly patterns
            self.maybe_switch_variant();

            encoded
        } else {
            self.encoder.encode(data)
        };

        LatentVector {
            components,
            dim_hint: self.dimension as u16,
            epoch: self.epoch,
        }
    }

    /// Decode data from the latent space
    pub fn decode(&mut self, vector: &LatentVector) -> Vec<u8> {
        self.encoder.decode(&vector.components, 0)
    }

    /// Evolve the key based on message history
    pub fn evolve(&mut self, message_hash: u64) {
        self.epoch += 1;

        // Use the quantum-resistant key state to generate a new morph seed
        let quantum_seed = self.quantum.get_morph_seed();

        // Combine with message hash for unique per-message evolution
        let combined_seed = quantum_seed ^ message_hash;

        // Perturb the neural encoder with the combined seed to shift the latent space
        self.encoder.evolve(combined_seed);
    }

    /// Get current anomaly level (average surprise over recent messages)
    pub fn anomaly_level(&self) -> f32 {
        if self.anomaly_scores.is_empty() {
            return 0.0;
        }
        let sum: f32 = self.anomaly_scores.iter().sum();
        sum / self.anomaly_scores.len() as f32
    }

    /// Get current MIRAS variant
    pub fn variant(&self) -> &'static str {
        match &self.miras_encoder {
            Some(enc) => enc.variant(),
            None => "Legacy",
        }
    }

    /// Adaptively switch MIRAS variant based on traffic patterns
    fn maybe_switch_variant(&mut self) {
        if self.miras_encoder.is_none() {
            return;
        }

        let anomaly = self.anomaly_level();
        let current = self.active_variant;

        // Decision logic for variant switching
        let new_variant = if anomaly > self.anomaly_threshold * 2.0 {
            // Very high anomaly → YAAD for outlier robustness
            MirasVariant::Yaad
        } else if anomaly > self.anomaly_threshold {
            // Moderate anomaly → MEMORA for balanced updates
            MirasVariant::Memora
        } else if self.epoch > 10000 {
            // Long-running session → MONETA for stability
            MirasVariant::Moneta { p: 2.0 }
        } else {
            // Normal operation → Titans baseline
            MirasVariant::Titans
        };

        // Only switch if variant changed (avoid unnecessary rebuilds)
        if std::mem::discriminant(&new_variant) != std::mem::discriminant(&current) {
            let seed = self.epoch ^ 0xDEADBEEF;
            let config = NeuralEncoderConfig {
                input_dim: 256,
                latent_dim: self.dimension,
                hidden_dims: vec![256, 128],
                attention_heads: 8,
                seed,
                miras_variant: new_variant,
                memory_tokens: 16,
            };
            self.miras_encoder = Some(config.build_miras());
            self.active_variant = new_variant;
        }
    }
}

/// Unified connection that works over both TCP and QUIC
pub struct SpineConnection {
    transport: TransportInner,
    handler: ProtocolHandlerState,
}

enum TransportInner {
    Tcp {
        stream: Box<dyn AsyncReadWrite>,
    },
    #[cfg(feature = "quic")]
    Quic {
        transport: QuicTransport,
    },
}

struct ProtocolHandlerState {
    cipher: Option<Aes256Gcm>,
    chameleon: Option<ChameleonKey>,
    morphology: ProtocolMorphology,
    moving_target: bool,
    /// Latent-space AEAD mode (M2)
    latent_aead: bool,
}

impl SpineConnection {
    /// Create from TCP stream
    pub fn from_tcp<S: AsyncReadWrite + 'static>(stream: S) -> Self {
        Self {
            transport: TransportInner::Tcp {
                stream: Box::new(stream),
            },
            handler: ProtocolHandlerState::new(),
        }
    }

    /// Create from QUIC transport
    #[cfg(feature = "quic")]
    pub fn from_quic(transport: QuicTransport) -> Self {
        Self {
            transport: TransportInner::Quic { transport },
            handler: ProtocolHandlerState::new(),
        }
    }

    /// Enable Chameleon protocol
    pub fn enable_chameleon(&mut self, secret: [u8; 32]) {
        self.handler.chameleon = Some(ChameleonKey::new(&secret));
        self.handler.morphology = ProtocolMorphology::new_keyed(
            u64::from_le_bytes(secret[0..8].try_into().unwrap()),
            &secret,
        );
        self.handler.moving_target = true;
    }

    /// Enable Chameleon + AES-GCM (M2: latent-space AEAD)
    pub fn enable_chameleon_aead(&mut self, secret: [u8; 32]) {
        let hk = hkdf::Hkdf::<sha2::Sha256>::new(None, &secret);
        let mut aes_key = [0u8; 32];
        hk.expand(b"spine-latent-aead-key", &mut aes_key)
            .expect("HKDF expand");

        self.handler.chameleon = Some(ChameleonKey::new(&secret));
        self.handler.morphology = ProtocolMorphology::new_keyed(
            u64::from_le_bytes(secret[0..8].try_into().unwrap()),
            &secret,
        );
        self.handler.moving_target = true;

        let key = aes_gcm::Key::<Aes256Gcm>::from_slice(&aes_key);
        self.handler.cipher = Some(Aes256Gcm::new(key));
        self.handler.latent_aead = true;
    }

    /// Get transport mode
    pub fn transport_mode(&self) -> TransportMode {
        match &self.transport {
            TransportInner::Tcp { .. } => TransportMode::Tcp,
            #[cfg(feature = "quic")]
            TransportInner::Quic { .. } => TransportMode::Quic,
        }
    }

    /// Get RTT estimate (QUIC only, returns None for TCP)
    #[cfg(feature = "quic")]
    pub fn rtt(&self) -> Option<std::time::Duration> {
        match &self.transport {
            TransportInner::Quic { transport } => Some(transport.rtt()),
            _ => None,
        }
    }

    /// Send a message
    pub async fn send(&mut self, msg: &Message) -> anyhow::Result<()> {
        let data = self.handler.encode_message(msg)?;

        match &mut self.transport {
            TransportInner::Tcp { stream } => {
                self.handler.write_frame(stream.as_mut(), &data).await?;
            }
            #[cfg(feature = "quic")]
            TransportInner::Quic { transport } => {
                // QUIC: length-prefixed frame. QuicTransport has its own
                // write_all; can't use the AsyncWrite-based vectored helper here.
                let len = (data.len() as u32).to_le_bytes();
                transport.write_all(&len).await?;
                transport.write_all(&data).await?;
            }
        }

        // Evolve key after sending
        if let Some(ref mut chameleon) = self.handler.chameleon {
            if self.handler.moving_target {
                let hash = Self::hash_data(&data);
                chameleon.evolve(hash);
                self.handler.morphology.evolve(hash);
            }
        }

        Ok(())
    }

    /// Receive a message
    pub async fn recv(&mut self) -> anyhow::Result<Message> {
        let data = match &mut self.transport {
            TransportInner::Tcp { stream } => self.handler.read_frame(stream.as_mut()).await?,
            #[cfg(feature = "quic")]
            TransportInner::Quic { transport } => {
                let mut len_buf = [0u8; 4];
                transport.read_exact(&mut len_buf).await?;
                let len = u32::from_le_bytes(len_buf) as usize;
                // The length prefix is peer-controlled; bound it before allocating.
                if len > wire::MAX_TRANSPORT_FRAME_BYTES {
                    anyhow::bail!(
                        "QUIC frame of {len} bytes exceeds the {} byte limit",
                        wire::MAX_TRANSPORT_FRAME_BYTES
                    );
                }
                let mut buf = vec![0u8; len];
                transport.read_exact(&mut buf).await?;
                buf
            }
        };

        let msg = self.handler.decode_message(&data)?;

        // Evolve key after receiving
        if let Some(ref mut chameleon) = self.handler.chameleon {
            if self.handler.moving_target {
                let hash = Self::hash_data(&data);
                chameleon.evolve(hash);
                self.handler.morphology.evolve(hash);
            }
        }

        Ok(msg)
    }

    fn hash_data(data: &[u8]) -> u64 {
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        hasher.finish()
    }
}

impl ProtocolHandlerState {
    fn new() -> Self {
        Self {
            cipher: None,
            chameleon: None,
            morphology: ProtocolMorphology::new(0),
            moving_target: false,
            latent_aead: false,
        }
    }

    fn encode_message(&mut self, msg: &Message) -> anyhow::Result<Vec<u8>> {
        let mut data = wire::encode(msg)?;

        // Apply padding
        data = self.apply_padding(data);

        // Adaptive compression with flag
        // SECURITY NOTE (L2): Compressing plaintext before encryption can leak
        // information via ciphertext size changes (CRIME/BREACH attacks). This is
        // acceptable for agent-to-agent communication where there is no reflected
        // user-controlled content. Disable compression when handling untrusted input.
        if data.len() >= 64 {
            let compressed = encode_all(&data[..], 3)?;
            let mut flagged = Vec::with_capacity(1 + compressed.len());
            flagged.push(0x01);
            flagged.extend_from_slice(&compressed);
            data = flagged;
        } else {
            data.insert(0, 0x00);
        }

        // Chameleon encoding
        if let Some(ref mut chameleon) = self.chameleon {
            let latent = chameleon.encode(&data);
            // FAST: Zero-copy binary serialization
            data = latent.to_bytes_fast();

            // M2: Latent-space AEAD
            if self.latent_aead {
                if let Some(cipher) = &self.cipher {
                    let nonce_full = fresh_nonce();
                    let nonce = Nonce::from_slice(&nonce_full);
                    data = cipher
                        .encrypt(nonce, data.as_ref())
                        .map_err(|e| anyhow::anyhow!("Latent AEAD encrypt error: {}", e))?;
                    let mut with_nonce = nonce_full.to_vec();
                    with_nonce.extend(data);
                    data = with_nonce;
                }
            }
        } else if let Some(cipher) = &self.cipher {
            let nonce_bytes = rand::random::<[u8; 12]>();
            let nonce = Nonce::from_slice(&nonce_bytes);
            data = cipher
                .encrypt(nonce, data.as_ref())
                .map_err(|e| anyhow::anyhow!("Encryption error: {}", e))?;
            let mut with_nonce = nonce_bytes.to_vec();
            with_nonce.extend(data);
            data = with_nonce;
        }

        Ok(data)
    }

    fn decode_message(&mut self, data: &[u8]) -> anyhow::Result<Message> {
        let mut data = data.to_vec();

        // Chameleon decoding
        if let Some(ref mut chameleon) = self.chameleon {
            // M2: Latent-space AEAD — decrypt before decoding
            if self.latent_aead {
                if let Some(cipher) = &self.cipher {
                    if data.len() < 12 {
                        return Err(anyhow::anyhow!("Invalid latent AEAD message"));
                    }
                    let nonce = Nonce::from_slice(&data[..12]);
                    data = cipher
                        .decrypt(nonce, &data[12..])
                        .map_err(|e| anyhow::anyhow!("Latent AEAD decrypt error: {}", e))?;
                }
            }

            // FAST: Zero-copy binary deserialization
            let latent = LatentVector::from_bytes_fast(&data)
                .ok_or_else(|| anyhow::anyhow!("Invalid latent vector format"))?;
            data = chameleon.decode(&latent);
        } else if let Some(cipher) = &self.cipher {
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

        let msg: Message = wire::decode(&data)?;
        Ok(msg)
    }

    async fn write_frame<W: AsyncWrite + Unpin + ?Sized>(
        &self,
        writer: &mut W,
        data: &[u8],
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            data.len() <= wire::MAX_TRANSPORT_FRAME_BYTES,
            "native transport frame exceeds size limit"
        );
        let len = data.len() as u32;
        let mut header = Vec::with_capacity(self.morphology.header_size as usize);

        let len_bytes = if self.morphology.big_endian {
            len.to_be_bytes()
        } else {
            len.to_le_bytes()
        };
        header.extend_from_slice(&len_bytes);
        header.push(self.morphology.frame_version);

        while header.len() < self.morphology.header_size as usize {
            header.push(self.morphology.checksum_variant);
        }

        write_header_body(writer, &header, data).await?;
        Ok(())
    }

    async fn read_frame<R: AsyncRead + Unpin + ?Sized>(
        &self,
        reader: &mut R,
    ) -> anyhow::Result<Vec<u8>> {
        let mut header = vec![0u8; self.morphology.header_size as usize];
        reader.read_exact(&mut header).await?;

        let len_bytes: [u8; 4] = header[0..4].try_into()?;
        let len = if self.morphology.big_endian {
            u32::from_be_bytes(len_bytes)
        } else {
            u32::from_le_bytes(len_bytes)
        } as usize;

        anyhow::ensure!(
            len <= wire::MAX_TRANSPORT_FRAME_BYTES,
            "native transport frame exceeds size limit"
        );
        let mut buf = vec![0u8; len];
        reader.read_exact(&mut buf).await?;
        Ok(buf)
    }

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
                data.push(padding_len as u8);
                data
            }
            PaddingMode::Chaotic => {
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
                data.extend_from_slice(&(original_len as u32).to_le_bytes());
                data
            }
        }
    }

    fn remove_padding(&self, mut data: Vec<u8>) -> Vec<u8> {
        match self.morphology.padding_mode {
            PaddingMode::None => data,
            PaddingMode::Pkcs7 => {
                if let Some(&padding_len) = data.last() {
                    let len = data.len().saturating_sub(padding_len as usize);
                    data.truncate(len);
                }
                data
            }
            PaddingMode::Random => {
                if let Some(&padding_len) = data.last() {
                    let len = data.len().saturating_sub(padding_len as usize + 1);
                    data.truncate(len);
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
}

/// Connect with automatic transport selection (QUIC with TCP fallback)
#[cfg(feature = "quic")]
pub async fn connect_auto(
    addr: SocketAddr,
    tcp_fallback: impl std::future::Future<Output = anyhow::Result<impl AsyncReadWrite + 'static>>,
    config: TransportConfig,
) -> anyhow::Result<SpineConnection> {
    match config.mode {
        TransportMode::Tcp => {
            let stream = tcp_fallback.await?;
            Ok(SpineConnection::from_tcp(stream))
        }
        TransportMode::Quic => {
            let builder = QuicEndpointBuilder::with_config(config.clone());
            let endpoint = builder.build_client()?;
            let server_name = config.server_name.as_deref().unwrap_or("localhost");
            let transport = builder.connect(&endpoint, addr, server_name).await?;
            Ok(SpineConnection::from_quic(transport))
        }
        TransportMode::Auto => {
            // Try QUIC first
            let builder = QuicEndpointBuilder::with_config(config.clone());
            if let Ok(endpoint) = builder.build_client() {
                let server_name = config.server_name.as_deref().unwrap_or("localhost");
                if let Ok(Ok(transport)) = tokio::time::timeout(
                    std::time::Duration::from_secs(2),
                    builder.connect(&endpoint, addr, server_name),
                )
                .await
                {
                    tracing::info!("Connected via QUIC to {}", addr);
                    return Ok(SpineConnection::from_quic(transport));
                }
            }

            // Fallback to TCP
            tracing::info!("QUIC unavailable, falling back to TCP for {}", addr);
            let stream = tcp_fallback.await?;
            Ok(SpineConnection::from_tcp(stream))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chameleon_key_basic() {
        let secret = [0x42u8; 32];
        let mut key = ChameleonKey::new(&secret);

        // Encode some data
        let data = b"Hello, Chameleon Protocol!";
        let vector = key.encode(data);

        assert!(!vector.components.is_empty());
        assert_eq!(vector.epoch, 0);

        // Evolve the key
        key.evolve(0xDEADBEEF);
        let vector2 = key.encode(data);
        assert_eq!(vector2.epoch, 1);
    }

    #[test]
    fn test_chameleon_key_miras() {
        let secret = [0x42u8; 32];
        let mut key = ChameleonKey::new_with_miras(&secret, MirasVariant::Yaad);

        assert_eq!(key.variant(), "YAAD");

        // Encode data
        let data = b"MIRAS-powered encoding";
        let vector = key.encode(data);
        assert!(!vector.components.is_empty());

        // Check anomaly tracking
        let anomaly = key.anomaly_level();
        assert!(anomaly >= 0.0);
    }

    #[test]
    fn test_chameleon_miras_variants() {
        let secret = [0x42u8; 32];

        // Test all MIRAS variants
        for variant in [
            MirasVariant::Titans,
            MirasVariant::Yaad,
            MirasVariant::Moneta { p: 2.0 },
            MirasVariant::Memora,
        ] {
            let mut key = ChameleonKey::new_with_miras(&secret, variant);
            let vector = key.encode(b"Test message");
            assert!(!vector.components.is_empty());
        }
    }

    /// The derivation has to be a function of the secret alone, or two peers
    /// sharing a secret would key their encoders differently and never agree.
    #[test]
    fn the_chameleon_seed_is_determined_by_the_secret() {
        let secret = [7u8; 32];
        assert_eq!(chameleon_seed(&secret), chameleon_seed(&secret));
    }

    /// And it has to actually depend on the secret. A derivation that ignored
    /// part of its input would silently shrink the key space further than the
    /// 64-bit output already does.
    #[test]
    fn every_byte_of_the_secret_changes_the_chameleon_seed() {
        let base = [0u8; 32];
        let baseline = chameleon_seed(&base);
        for position in 0..32 {
            let mut altered = base;
            altered[position] ^= 0xff;
            assert_ne!(
                chameleon_seed(&altered),
                baseline,
                "byte {position} of the secret does not reach the seed"
            );
        }
    }

    /// The seed must not be recoverable by a hash anyone can compute. This
    /// pins the derivation to HKDF rather than to the `DefaultHasher` it
    /// replaced — which was not merely weak but *unstable*, since the standard
    /// library does not promise its algorithm across Rust releases, so two
    /// peers on different compilers could disagree about the same secret.
    #[test]
    fn the_chameleon_seed_is_not_the_std_hash_of_the_secret() {
        use std::hash::{Hash, Hasher};
        let secret = [3u8; 32];
        let mut hasher = DefaultHasher::new();
        secret.hash(&mut hasher);
        assert_ne!(
            chameleon_seed(&secret),
            hasher.finish(),
            "the seed must come from a KDF, not from a HashMap hasher"
        );
    }
}
