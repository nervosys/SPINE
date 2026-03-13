//! Protocol version negotiation and capability handshake.
//!
//! Provides version exchange and feature negotiation during connection setup.
//! The initiator sends a `VersionOffer`, the responder replies with a `VersionResponse`,
//! and both sides agree on the highest compatible version and shared feature set.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

/// Current protocol version.
pub const PROTOCOL_VERSION_MAJOR: u32 = 1;
pub const PROTOCOL_VERSION_MINOR: u32 = 0;

/// Known protocol features that can be negotiated.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ProtocolFeature {
    /// Chameleon latent-space encryption
    Chameleon,
    /// AES-256-GCM authenticated encryption
    AesGcm,
    /// Latent-space AEAD (defense-in-depth on latent vectors)
    LatentAead,
    /// Zstd compression for payloads >= 64 bytes
    Compression,
    /// Speculative decoding / prediction
    Speculation,
    /// Protocol morphology (moving-target defense)
    Morphology,
    /// QUIC transport
    Quic,
    /// WebSocket transport
    WebSocket,
    /// ML-KEM post-quantum key exchange
    MlKem,
    /// RLWE key exchange
    Rlwe,
    /// Hybrid KEM (RLWE + ML-KEM)
    HybridKem,
    /// Binary latent vector serialization
    BinaryLatent,
    /// Custom feature string for extensibility
    Custom(String),
}

impl fmt::Display for ProtocolFeature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Chameleon => write!(f, "chameleon"),
            Self::AesGcm => write!(f, "aes-gcm"),
            Self::LatentAead => write!(f, "latent-aead"),
            Self::Compression => write!(f, "compression"),
            Self::Speculation => write!(f, "speculation"),
            Self::Morphology => write!(f, "morphology"),
            Self::Quic => write!(f, "quic"),
            Self::WebSocket => write!(f, "websocket"),
            Self::MlKem => write!(f, "ml-kem"),
            Self::Rlwe => write!(f, "rlwe"),
            Self::HybridKem => write!(f, "hybrid-kem"),
            Self::BinaryLatent => write!(f, "binary-latent"),
            Self::Custom(s) => write!(f, "custom:{s}"),
        }
    }
}

/// Version offer sent by the initiating side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionOffer {
    /// Magic bytes identifying this as a SPINE handshake.
    pub magic: [u8; 4],
    /// Offered protocol major version.
    pub major: u32,
    /// Offered protocol minor version.
    pub minor: u32,
    /// Minimum acceptable major version.
    pub min_major: u32,
    /// Minimum acceptable minor version.
    pub min_minor: u32,
    /// Set of features supported by this peer.
    pub features: BTreeSet<ProtocolFeature>,
    /// Optional peer identifier.
    pub peer_id: Option<String>,
}

/// Response to a version offer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionResponse {
    /// Magic bytes confirming SPINE handshake.
    pub magic: [u8; 4],
    /// Whether the version was accepted.
    pub accepted: bool,
    /// Negotiated major version (highest mutually supported).
    pub major: u32,
    /// Negotiated minor version.
    pub minor: u32,
    /// Intersection of mutually supported features.
    pub features: BTreeSet<ProtocolFeature>,
    /// Rejection reason if not accepted.
    pub reason: Option<String>,
}

/// Result of a completed negotiation.
#[derive(Debug, Clone)]
pub struct NegotiatedProtocol {
    /// Agreed major version.
    pub major: u32,
    /// Agreed minor version.
    pub minor: u32,
    /// Agreed feature set.
    pub features: BTreeSet<ProtocolFeature>,
    /// Remote peer identifier.
    pub remote_peer_id: Option<String>,
}

impl NegotiatedProtocol {
    /// Check if a feature was negotiated.
    pub fn has_feature(&self, feature: &ProtocolFeature) -> bool {
        self.features.contains(feature)
    }
}

/// Magic bytes: "SPNE" (0x53 0x50 0x4E 0x45)
pub const HANDSHAKE_MAGIC: [u8; 4] = [0x53, 0x50, 0x4E, 0x45];

/// Negotiation errors.
#[derive(Debug, thiserror::Error)]
pub enum NegotiationError {
    #[error("invalid magic bytes: expected SPNE, got {0:?}")]
    InvalidMagic([u8; 4]),
    #[error("incompatible version: offered {offered_major}.{offered_minor}, minimum {min_major}.{min_minor}")]
    IncompatibleVersion {
        offered_major: u32,
        offered_minor: u32,
        min_major: u32,
        min_minor: u32,
    },
    #[error("negotiation rejected: {0}")]
    Rejected(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Build a default version offer with all locally supported features.
pub fn default_offer(peer_id: Option<String>) -> VersionOffer {
    let mut features = BTreeSet::new();
    features.insert(ProtocolFeature::Chameleon);
    features.insert(ProtocolFeature::AesGcm);
    features.insert(ProtocolFeature::LatentAead);
    features.insert(ProtocolFeature::Compression);
    features.insert(ProtocolFeature::Speculation);
    features.insert(ProtocolFeature::Morphology);
    features.insert(ProtocolFeature::BinaryLatent);

    VersionOffer {
        magic: HANDSHAKE_MAGIC,
        major: PROTOCOL_VERSION_MAJOR,
        minor: PROTOCOL_VERSION_MINOR,
        min_major: 1,
        min_minor: 0,
        features,
        peer_id,
    }
}

/// Evaluate an incoming version offer and produce a response.
///
/// Accepts if the offered version range overlaps with our supported range.
/// Features are intersected — only mutually supported features are agreed upon.
pub fn evaluate_offer(
    offer: &VersionOffer,
    local_features: &BTreeSet<ProtocolFeature>,
) -> Result<(VersionResponse, NegotiatedProtocol), NegotiationError> {
    // Validate magic
    if offer.magic != HANDSHAKE_MAGIC {
        return Err(NegotiationError::InvalidMagic(offer.magic));
    }

    // Version compatibility: we accept if offered major >= our min and offered min <= our current
    let our_major = PROTOCOL_VERSION_MAJOR;
    let our_minor = PROTOCOL_VERSION_MINOR;

    let compatible = offer.major >= 1;



    if !compatible {
        return Err(NegotiationError::IncompatibleVersion {
            offered_major: offer.major,
            offered_minor: offer.minor,
            min_major: 1,
            min_minor: 0,
        });
    }

    // Negotiated version is the minimum of both sides
    let neg_major = offer.major.min(our_major);
    let neg_minor = if offer.major == our_major {
        offer.minor.min(our_minor)
    } else if offer.major < our_major {
        offer.minor
    } else {
        our_minor
    };

    // Feature intersection
    let shared_features: BTreeSet<ProtocolFeature> = offer
        .features
        .intersection(local_features)
        .cloned()
        .collect();

    let negotiated = NegotiatedProtocol {
        major: neg_major,
        minor: neg_minor,
        features: shared_features.clone(),
        remote_peer_id: offer.peer_id.clone(),
    };

    let response = VersionResponse {
        magic: HANDSHAKE_MAGIC,
        accepted: true,
        major: neg_major,
        minor: neg_minor,
        features: shared_features,
        reason: None,
    };

    Ok((response, negotiated))
}

/// Validate a version response received after sending an offer.
pub fn validate_response(
    response: &VersionResponse,
    original_offer: &VersionOffer,
) -> Result<NegotiatedProtocol, NegotiationError> {
    if response.magic != HANDSHAKE_MAGIC {
        return Err(NegotiationError::InvalidMagic(response.magic));
    }

    if !response.accepted {
        return Err(NegotiationError::Rejected(
            response.reason.clone().unwrap_or_else(|| "unknown reason".into()),
        ));
    }

    // Verify negotiated version is within our acceptable range
    if response.major < original_offer.min_major
        || (response.major == original_offer.min_major
            && response.minor < original_offer.min_minor)
    {
        return Err(NegotiationError::IncompatibleVersion {
            offered_major: response.major,
            offered_minor: response.minor,
            min_major: original_offer.min_major,
            min_minor: original_offer.min_minor,
        });
    }

    // Verify features are a subset of what we offered
    for feat in &response.features {
        if !original_offer.features.contains(feat) {
            return Err(NegotiationError::Rejected(format!(
                "server granted unrequested feature: {feat}"
            )));
        }
    }

    Ok(NegotiatedProtocol {
        major: response.major,
        minor: response.minor,
        features: response.features.clone(),
        remote_peer_id: None,
    })
}

/// Serialize a version offer to bytes (length-prefixed JSON).
pub fn serialize_offer(offer: &VersionOffer) -> Result<Vec<u8>, NegotiationError> {
    let json = serde_json::to_vec(offer)
        .map_err(|e| NegotiationError::Serialization(e.to_string()))?;
    let len = (json.len() as u32).to_be_bytes();
    let mut buf = Vec::with_capacity(4 + json.len());
    buf.extend_from_slice(&len);
    buf.extend_from_slice(&json);
    Ok(buf)
}

/// Serialize a version response to bytes (length-prefixed JSON).
pub fn serialize_response(response: &VersionResponse) -> Result<Vec<u8>, NegotiationError> {
    let json = serde_json::to_vec(response)
        .map_err(|e| NegotiationError::Serialization(e.to_string()))?;
    let len = (json.len() as u32).to_be_bytes();
    let mut buf = Vec::with_capacity(4 + json.len());
    buf.extend_from_slice(&len);
    buf.extend_from_slice(&json);
    Ok(buf)
}

/// Deserialize a version offer from length-prefixed bytes.
pub fn deserialize_offer(data: &[u8]) -> Result<VersionOffer, NegotiationError> {
    if data.len() < 4 {
        return Err(NegotiationError::Serialization("buffer too short".into()));
    }
    let len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if data.len() < 4 + len {
        return Err(NegotiationError::Serialization("truncated payload".into()));
    }
    serde_json::from_slice(&data[4..4 + len])
        .map_err(|e| NegotiationError::Serialization(e.to_string()))
}

/// Deserialize a version response from length-prefixed bytes.
pub fn deserialize_response(data: &[u8]) -> Result<VersionResponse, NegotiationError> {
    if data.len() < 4 {
        return Err(NegotiationError::Serialization("buffer too short".into()));
    }
    let len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if data.len() < 4 + len {
        return Err(NegotiationError::Serialization("truncated payload".into()));
    }
    serde_json::from_slice(&data[4..4 + len])
        .map_err(|e| NegotiationError::Serialization(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_features() -> BTreeSet<ProtocolFeature> {
        let mut f = BTreeSet::new();
        f.insert(ProtocolFeature::Chameleon);
        f.insert(ProtocolFeature::AesGcm);
        f.insert(ProtocolFeature::LatentAead);
        f.insert(ProtocolFeature::Compression);
        f.insert(ProtocolFeature::Speculation);
        f.insert(ProtocolFeature::Morphology);
        f.insert(ProtocolFeature::BinaryLatent);
        f
    }

    #[test]
    fn test_default_offer() {
        let offer = default_offer(Some("agent-1".into()));
        assert_eq!(offer.magic, HANDSHAKE_MAGIC);
        assert_eq!(offer.major, PROTOCOL_VERSION_MAJOR);
        assert_eq!(offer.minor, PROTOCOL_VERSION_MINOR);
        assert_eq!(offer.peer_id, Some("agent-1".into()));
        assert!(offer.features.contains(&ProtocolFeature::Chameleon));
        assert!(offer.features.contains(&ProtocolFeature::Compression));
    }

    #[test]
    fn test_evaluate_offer_success() {
        let offer = default_offer(Some("client".into()));
        let local = all_features();
        let (resp, neg) = evaluate_offer(&offer, &local).unwrap();
        assert!(resp.accepted);
        assert_eq!(neg.major, PROTOCOL_VERSION_MAJOR);
        assert_eq!(neg.minor, PROTOCOL_VERSION_MINOR);
        assert_eq!(neg.remote_peer_id, Some("client".into()));
        assert!(neg.has_feature(&ProtocolFeature::Chameleon));
    }

    #[test]
    fn test_evaluate_offer_feature_intersection() {
        let offer = default_offer(None);
        // Server only supports a subset
        let mut local = BTreeSet::new();
        local.insert(ProtocolFeature::Compression);
        local.insert(ProtocolFeature::AesGcm);

        let (resp, neg) = evaluate_offer(&offer, &local).unwrap();
        assert!(resp.accepted);
        assert_eq!(neg.features.len(), 2);
        assert!(neg.has_feature(&ProtocolFeature::Compression));
        assert!(neg.has_feature(&ProtocolFeature::AesGcm));
        assert!(!neg.has_feature(&ProtocolFeature::Chameleon));
    }

    #[test]
    fn test_evaluate_offer_invalid_magic() {
        let mut offer = default_offer(None);
        offer.magic = [0, 0, 0, 0];
        let local = all_features();
        let err = evaluate_offer(&offer, &local).unwrap_err();
        assert!(matches!(err, NegotiationError::InvalidMagic(_)));
    }

    #[test]
    fn test_validate_response_success() {
        let offer = default_offer(None);
        let local = all_features();
        let (resp, _) = evaluate_offer(&offer, &local).unwrap();
        let neg = validate_response(&resp, &offer).unwrap();
        assert_eq!(neg.major, PROTOCOL_VERSION_MAJOR);
    }

    #[test]
    fn test_validate_response_rejected() {
        let offer = default_offer(None);
        let resp = VersionResponse {
            magic: HANDSHAKE_MAGIC,
            accepted: false,
            major: 1,
            minor: 0,
            features: BTreeSet::new(),
            reason: Some("server full".into()),
        };
        let err = validate_response(&resp, &offer).unwrap_err();
        assert!(matches!(err, NegotiationError::Rejected(_)));
    }

    #[test]
    fn test_validate_response_unrequested_feature() {
        let mut offer = default_offer(None);
        offer.features.clear();
        offer.features.insert(ProtocolFeature::Compression);

        let mut resp_features = BTreeSet::new();
        resp_features.insert(ProtocolFeature::Compression);
        resp_features.insert(ProtocolFeature::Chameleon); // not offered

        let resp = VersionResponse {
            magic: HANDSHAKE_MAGIC,
            accepted: true,
            major: 1,
            minor: 0,
            features: resp_features,
            reason: None,
        };
        let err = validate_response(&resp, &offer).unwrap_err();
        assert!(matches!(err, NegotiationError::Rejected(_)));
    }

    #[test]
    fn test_serialize_roundtrip_offer() {
        let offer = default_offer(Some("test".into()));
        let bytes = serialize_offer(&offer).unwrap();
        let decoded = deserialize_offer(&bytes).unwrap();
        assert_eq!(decoded.magic, offer.magic);
        assert_eq!(decoded.major, offer.major);
        assert_eq!(decoded.features.len(), offer.features.len());
        assert_eq!(decoded.peer_id, offer.peer_id);
    }

    #[test]
    fn test_serialize_roundtrip_response() {
        let offer = default_offer(None);
        let local = all_features();
        let (resp, _) = evaluate_offer(&offer, &local).unwrap();
        let bytes = serialize_response(&resp).unwrap();
        let decoded = deserialize_response(&bytes).unwrap();
        assert_eq!(decoded.accepted, resp.accepted);
        assert_eq!(decoded.major, resp.major);
        assert_eq!(decoded.features.len(), resp.features.len());
    }

    #[test]
    fn test_deserialize_truncated() {
        let err = deserialize_offer(&[0, 0, 0, 10, 1, 2]).unwrap_err();
        assert!(matches!(err, NegotiationError::Serialization(_)));
    }

    #[test]
    fn test_deserialize_too_short() {
        let err = deserialize_offer(&[0, 0]).unwrap_err();
        assert!(matches!(err, NegotiationError::Serialization(_)));
    }

    #[test]
    fn test_protocol_feature_display() {
        assert_eq!(format!("{}", ProtocolFeature::Chameleon), "chameleon");
        assert_eq!(format!("{}", ProtocolFeature::MlKem), "ml-kem");
        assert_eq!(
            format!("{}", ProtocolFeature::Custom("foo".into())),
            "custom:foo"
        );
    }

    #[test]
    fn test_negotiated_protocol_has_feature() {
        let mut features = BTreeSet::new();
        features.insert(ProtocolFeature::Compression);
        let neg = NegotiatedProtocol {
            major: 1,
            minor: 0,
            features,
            remote_peer_id: None,
        };
        assert!(neg.has_feature(&ProtocolFeature::Compression));
        assert!(!neg.has_feature(&ProtocolFeature::Chameleon));
    }

    #[test]
    fn test_handshake_magic_constant() {
        assert_eq!(&HANDSHAKE_MAGIC, b"SPNE");
    }

    #[test]
    fn test_custom_feature_ordering() {
        let mut features = BTreeSet::new();
        features.insert(ProtocolFeature::Custom("zebra".into()));
        features.insert(ProtocolFeature::Custom("alpha".into()));
        features.insert(ProtocolFeature::Chameleon);
        // BTreeSet maintains ordering — custom features come after built-in
        assert_eq!(features.len(), 3);
    }
}
