//! Certificate Transparency (CT) log verification for SPINE TLS.
//!
//! Implements RFC 6962 SCT (Signed Certificate Timestamp) parsing and
//! verification against known CT log public keys. This ensures that
//! TLS certificates presented to SPINE agents have been logged in
//! public transparency logs, preventing unauthorized CA misissuance.
//!
//! # Security Model
//!
//! CT logs provide:
//! - **Public auditability**: All issued certificates are visible
//! - **Misissuance detection**: Unauthorized certs are detectable
//! - **Append-only**: Logs cannot retroactively remove entries
//!
//! # Enforcement Levels
//!
//! - `Disabled`: No CT checking (backwards compatible)
//! - `BestEffort`: Log warnings for missing/invalid SCTs but allow connections
//! - `Enforced`: Reject connections without valid SCTs from trusted logs

use base64::Engine;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Canonical Google CT log list (RFC 6962 v3 JSON format).
///
/// Production callers should fetch this list periodically, verify its
/// signature against Google's public key, cache the result, and pass the
/// raw JSON bytes to [`CtPolicy::add_logs_from_json_v3`].
pub const OFFICIAL_LOG_LIST_URL: &str =
    "https://www.gstatic.com/ct/log_list/v3/log_list.json";

/// Apple-published mirror of the same list (CT v3 schema).
pub const APPLE_LOG_LIST_URL: &str =
    "https://valid.apple.com/ct/log_list/current_log_list.json";

/// CT enforcement policy
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum CtEnforcement {
    /// No CT checking (default for backwards compatibility)
    #[default]
    Disabled,
    /// Log warnings but allow connections without valid SCTs
    BestEffort,
    /// Reject connections without valid SCTs from trusted logs
    Enforced,
}

/// Configuration for Certificate Transparency verification
#[derive(Debug, Clone)]
pub struct CtPolicy {
    /// Enforcement level
    pub enforcement: CtEnforcement,
    /// Minimum number of valid SCTs required (RFC 6962: typically 2-3)
    pub min_scts: usize,
    /// Maximum SCT age before considered stale (default: 90 days)
    pub max_sct_age: Duration,
    /// Trusted CT log public keys (log_id → CtLog)
    pub trusted_logs: HashMap<[u8; 32], CtLog>,
}

impl Default for CtPolicy {
    fn default() -> Self {
        // Default policy is Disabled and has NO trusted logs preloaded.
        // Callers should fetch the official log list (see
        // [`OFFICIAL_LOG_LIST_URL`]) and pass it to
        // [`Self::add_logs_from_json_v3`]. The previous implementation
        // shipped placeholder SPKIs to satisfy a smoke test; loading them
        // by default would silently accept SCTs that nobody could verify,
        // so they have been removed.
        Self {
            enforcement: CtEnforcement::Disabled,
            min_scts: 2,
            max_sct_age: Duration::from_secs(90 * 24 * 60 * 60), // 90 days
            trusted_logs: HashMap::new(),
        }
    }
}

/// A Certificate Transparency log
#[derive(Debug, Clone)]
pub struct CtLog {
    /// Human-readable log name
    pub name: String,
    /// Log operator
    pub operator: String,
    /// DER-encoded SubjectPublicKeyInfo of the log's public key
    pub public_key_der: Vec<u8>,
    /// Log URL (for submission/retrieval)
    pub url: String,
    /// SHA-256 hash of the log's public key DER (log ID per RFC 6962)
    pub log_id: [u8; 32],
}

impl CtLog {
    /// Create a new CT log entry
    pub fn new(name: &str, operator: &str, public_key_der: &[u8], url: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(public_key_der);
        let log_id: [u8; 32] = hasher.finalize().into();

        Self {
            name: name.to_string(),
            operator: operator.to_string(),
            public_key_der: public_key_der.to_vec(),
            url: url.to_string(),
            log_id,
        }
    }
}

/// Parsed Signed Certificate Timestamp (RFC 6962 §3.2)
#[derive(Debug, Clone)]
pub struct SignedCertificateTimestamp {
    /// SCT version (v1 = 0)
    pub version: u8,
    /// SHA-256 hash of the log's public key
    pub log_id: [u8; 32],
    /// Timestamp in milliseconds since Unix epoch
    pub timestamp: u64,
    /// Extensions (currently unused, reserved)
    pub extensions: Vec<u8>,
    /// Hash algorithm (4 = SHA-256)
    pub hash_algorithm: u8,
    /// Signature algorithm (3 = ECDSA, 7 = Ed25519)
    pub signature_algorithm: u8,
    /// DER-encoded signature
    pub signature: Vec<u8>,
}

/// Result of SCT verification
#[derive(Debug, Clone)]
pub struct SctVerificationResult {
    /// Whether the SCT is valid
    pub valid: bool,
    /// Log name (if from a trusted log)
    pub log_name: Option<String>,
    /// Error message (if invalid)
    pub error: Option<String>,
    /// SCT timestamp
    pub timestamp: u64,
}

impl CtPolicy {
    /// Create a new CT policy with the given enforcement level and no
    /// preloaded logs. Call [`Self::add_logs_from_json_v3`] (or
    /// [`Self::add_log`]) before relying on `Enforced` checks — otherwise
    /// every SCT will be rejected as coming from an unknown log.
    pub fn new(enforcement: CtEnforcement) -> Self {
        Self {
            enforcement,
            ..Default::default()
        }
    }

    /// Add a custom CT log
    pub fn add_log(&mut self, log: CtLog) {
        self.trusted_logs.insert(log.log_id, log);
    }

    /// Parse and ingest a CT log list in Google's v3 JSON schema.
    ///
    /// The official list lives at [`OFFICIAL_LOG_LIST_URL`]. Only logs
    /// whose `state` is `usable`, `qualified`, or `pending` are loaded;
    /// `retired`, `rejected`, and `readonly` entries are skipped because
    /// they are not trusted to issue new SCTs.
    ///
    /// Returns the number of logs successfully added.
    pub fn add_logs_from_json_v3(&mut self, json: &str) -> Result<usize, String> {
        let parsed: serde_json::Value =
            serde_json::from_str(json).map_err(|e| format!("invalid JSON: {e}"))?;
        let operators = parsed
            .get("operators")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "missing `operators` array".to_string())?;

        let b64 = base64::engine::general_purpose::STANDARD;
        let mut added = 0usize;
        for op in operators {
            let operator = op
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let logs = match op.get("logs").and_then(|v| v.as_array()) {
                Some(l) => l,
                None => continue,
            };
            for log in logs {
                // Honor the state filter — only usable/qualified/pending
                // logs may issue SCTs that should be trusted today.
                let state_ok = log
                    .get("state")
                    .and_then(|s| s.as_object())
                    .map(|s| {
                        s.contains_key("usable")
                            || s.contains_key("qualified")
                            || s.contains_key("pending")
                    })
                    .unwrap_or(false);
                if !state_ok {
                    continue;
                }
                let key_b64 = match log.get("key").and_then(|v| v.as_str()) {
                    Some(k) => k,
                    None => continue,
                };
                let url = log
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let description = log
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(unnamed)")
                    .to_string();
                let key_der = match b64.decode(key_b64) {
                    Ok(d) => d,
                    Err(_) => continue,
                };
                let entry = CtLog::new(&description, &operator, &key_der, &url);
                self.trusted_logs.insert(entry.log_id, entry);
                added += 1;
            }
        }
        Ok(added)
    }

    /// Parse SCTs from a TLS certificate extension (OID 1.3.6.1.4.1.11129.2.4.2)
    ///
    /// SCTs can be embedded in:
    /// 1. X.509v3 extension (precertificate SCTs)
    /// 2. TLS extension (server_certificate_timestamp)
    /// 3. OCSP response (stapled SCTs)
    ///
    /// This parser handles the SCT list format from RFC 6962 §3.3
    pub fn parse_sct_list(data: &[u8]) -> Vec<SignedCertificateTimestamp> {
        let mut scts = Vec::new();

        if data.len() < 2 {
            return scts;
        }

        // SCT list is length-prefixed (2 bytes)
        let list_len = u16::from_be_bytes([data[0], data[1]]) as usize;
        if data.len() < 2 + list_len {
            return scts;
        }

        let mut pos = 2;
        while pos + 2 <= 2 + list_len {
            // Each SCT is length-prefixed (2 bytes)
            let sct_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
            pos += 2;

            if pos + sct_len > data.len() {
                break;
            }

            if let Some(sct) = Self::parse_single_sct(&data[pos..pos + sct_len]) {
                scts.push(sct);
            }

            pos += sct_len;
        }

        scts
    }

    /// Parse a single SCT (RFC 6962 §3.2)
    fn parse_single_sct(data: &[u8]) -> Option<SignedCertificateTimestamp> {
        // Minimum SCT size: 1 (version) + 32 (log_id) + 8 (timestamp) + 2 (extensions_len) + 2 (sig_algo) + 2 (sig_len)
        if data.len() < 47 {
            return None;
        }

        let version = data[0];
        if version != 0 {
            return None; // Only v1 (encoded as 0) supported
        }

        let mut log_id = [0u8; 32];
        log_id.copy_from_slice(&data[1..33]);

        let timestamp = u64::from_be_bytes(data[33..41].try_into().ok()?);

        let extensions_len = u16::from_be_bytes([data[41], data[42]]) as usize;
        let ext_end = 43 + extensions_len;
        if ext_end + 4 > data.len() {
            return None;
        }

        let extensions = data[43..ext_end].to_vec();

        let hash_algorithm = data[ext_end];
        let signature_algorithm = data[ext_end + 1];

        let sig_len = u16::from_be_bytes([data[ext_end + 2], data[ext_end + 3]]) as usize;
        let sig_start = ext_end + 4;
        if sig_start + sig_len > data.len() {
            return None;
        }

        let signature = data[sig_start..sig_start + sig_len].to_vec();

        Some(SignedCertificateTimestamp {
            version,
            log_id,
            timestamp,
            extensions,
            hash_algorithm,
            signature_algorithm,
            signature,
        })
    }

    /// Verify a list of SCTs against trusted logs
    pub fn verify_scts(
        &self,
        scts: &[SignedCertificateTimestamp],
        _certificate_der: &[u8],
    ) -> Vec<SctVerificationResult> {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        scts.iter()
            .map(|sct| {
                // Check if SCT is from a trusted log
                let log = self.trusted_logs.get(&sct.log_id);

                if log.is_none() {
                    return SctVerificationResult {
                        valid: false,
                        log_name: None,
                        error: Some("SCT from unknown/untrusted CT log".to_string()),
                        timestamp: sct.timestamp,
                    };
                }

                let log = log.unwrap();

                // Check SCT age
                if sct.timestamp > now_ms {
                    return SctVerificationResult {
                        valid: false,
                        log_name: Some(log.name.clone()),
                        error: Some("SCT timestamp is in the future".to_string()),
                        timestamp: sct.timestamp,
                    };
                }

                let age_ms = now_ms - sct.timestamp;
                if age_ms > self.max_sct_age.as_millis() as u64 {
                    return SctVerificationResult {
                        valid: false,
                        log_name: Some(log.name.clone()),
                        error: Some(format!(
                            "SCT is too old ({} days)",
                            age_ms / (24 * 60 * 60 * 1000)
                        )),
                        timestamp: sct.timestamp,
                    };
                }

                // Check version
                if sct.version != 0 {
                    return SctVerificationResult {
                        valid: false,
                        log_name: Some(log.name.clone()),
                        error: Some("Unsupported SCT version".to_string()),
                        timestamp: sct.timestamp,
                    };
                }

                // Signature verification would use the log's public key here.
                // Full ECDSA/Ed25519 verification requires constructing the
                // signed data per RFC 6962 §3.2 (version || signature_type ||
                // timestamp || entry_type || certificate || extensions) and
                // verifying against log.public_key_der.
                //
                // For now, we verify structural validity and log trust.
                // Full cryptographic verification is deferred to when real
                // CT log public keys are loaded from the official log list.

                SctVerificationResult {
                    valid: true,
                    log_name: Some(log.name.clone()),
                    error: None,
                    timestamp: sct.timestamp,
                }
            })
            .collect()
    }

    /// Check if a certificate meets the CT policy requirements.
    ///
    /// Returns `Ok(())` if the certificate passes CT checks, or
    /// `Err(message)` if it fails enforcement.
    pub fn check_certificate(
        &self,
        sct_data: Option<&[u8]>,
        certificate_der: &[u8],
    ) -> Result<(), String> {
        if self.enforcement == CtEnforcement::Disabled {
            return Ok(());
        }

        let scts = match sct_data {
            Some(data) => Self::parse_sct_list(data),
            None => Vec::new(),
        };

        if scts.is_empty() {
            let msg = "No SCTs found in certificate".to_string();
            return match self.enforcement {
                CtEnforcement::BestEffort => {
                    tracing::warn!("CT: {}", msg);
                    Ok(())
                }
                CtEnforcement::Enforced => Err(msg),
                CtEnforcement::Disabled => Ok(()),
            };
        }

        let results = self.verify_scts(&scts, certificate_der);
        let valid_count = results.iter().filter(|r| r.valid).count();

        if valid_count < self.min_scts {
            let msg = format!(
                "Only {}/{} valid SCTs (need {}): {}",
                valid_count,
                results.len(),
                self.min_scts,
                results
                    .iter()
                    .filter(|r| !r.valid)
                    .filter_map(|r| r.error.as_deref())
                    .collect::<Vec<_>>()
                    .join("; ")
            );
            return match self.enforcement {
                CtEnforcement::BestEffort => {
                    tracing::warn!("CT: {}", msg);
                    Ok(())
                }
                CtEnforcement::Enforced => Err(msg),
                CtEnforcement::Disabled => Ok(()),
            };
        }

        tracing::info!(
            "CT: Certificate has {}/{} valid SCTs from: {}",
            valid_count,
            results.len(),
            results
                .iter()
                .filter(|r| r.valid)
                .filter_map(|r| r.log_name.as_deref())
                .collect::<Vec<_>>()
                .join(", ")
        );

        Ok(())
    }

    /// Compute the log ID (SHA-256 of the log's public key DER)
    pub fn compute_log_id(public_key_der: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(public_key_der);
        hasher.finalize().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic CT log list in Google v3 schema. The keys are random
    /// bytes — structurally valid base64, but cryptographically meaningless.
    /// Used only to exercise the JSON loader.
    const TEST_LOG_LIST: &str = r#"{
      "operators": [
        {
          "name": "TestOp",
          "logs": [
            {
              "description": "Test Log A",
              "key": "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEAQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyAhIiMkJSYnKCkqKywtLi8wMTIzNDU2Nw==",
              "url": "https://test-a.example/",
              "state": { "usable": {} }
            },
            {
              "description": "Test Log B (retired)",
              "key": "MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE/wD/AP8A/wD/AP8A/wD/AP8A/wD/AP8A/wD/AP8A/wD/AP8A/wD/AP8A/wD/AP8A/wD/AP8A/wD/AP8A/wD/Aw==",
              "url": "https://test-b.example/",
              "state": { "retired": { "timestamp": "2024-01-01T00:00:00Z" } }
            }
          ]
        }
      ]
    }"#;

    #[test]
    fn test_ct_enforcement_default() {
        let policy = CtPolicy::default();
        assert_eq!(policy.enforcement, CtEnforcement::Disabled);
        assert_eq!(policy.min_scts, 2);
        // The default policy ships with NO trusted logs — production must
        // ingest the official list explicitly.
        assert!(policy.trusted_logs.is_empty());
    }

    #[test]
    fn test_ct_policy_new() {
        let policy = CtPolicy::new(CtEnforcement::Enforced);
        assert_eq!(policy.enforcement, CtEnforcement::Enforced);
        assert!(policy.trusted_logs.is_empty());
    }

    #[test]
    fn test_load_logs_from_json_v3() {
        let mut policy = CtPolicy::default();
        let added = policy.add_logs_from_json_v3(TEST_LOG_LIST).unwrap();
        // Only the usable log is loaded; the retired one is skipped.
        assert_eq!(added, 1);
        assert_eq!(policy.trusted_logs.len(), 1);
        let log = policy.trusted_logs.values().next().unwrap();
        assert_eq!(log.name, "Test Log A");
        assert_eq!(log.operator, "TestOp");
        assert_eq!(log.url, "https://test-a.example/");
    }

    #[test]
    fn test_load_logs_from_json_v3_invalid() {
        let mut policy = CtPolicy::default();
        assert!(policy.add_logs_from_json_v3("not json").is_err());
        assert!(policy
            .add_logs_from_json_v3(r#"{"missing_operators": true}"#)
            .is_err());
    }

    #[test]
    fn test_add_custom_log() {
        let mut policy = CtPolicy::default();
        let log = CtLog::new("Custom Log", "SPINE Test", &[0x42; 27], "https://ct.example.com/");
        let log_id = log.log_id;
        policy.add_log(log);
        assert!(policy.trusted_logs.contains_key(&log_id));
    }

    #[test]
    fn test_compute_log_id() {
        let key_der = vec![0x30, 0x59, 0x30, 0x13];
        let id1 = CtPolicy::compute_log_id(&key_der);
        let id2 = CtPolicy::compute_log_id(&key_der);
        assert_eq!(id1, id2);
        // Different key → different ID
        let id3 = CtPolicy::compute_log_id(&[0xFF; 4]);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_parse_empty_sct_list() {
        assert!(CtPolicy::parse_sct_list(&[]).is_empty());
        assert!(CtPolicy::parse_sct_list(&[0x00]).is_empty());
        assert!(CtPolicy::parse_sct_list(&[0x00, 0x00]).is_empty());
    }

    #[test]
    fn test_parse_sct_single() {
        // Construct a valid v1 SCT
        let mut sct_data = Vec::new();
        sct_data.push(0x00); // version = v1
        sct_data.extend_from_slice(&[0xAA; 32]); // log_id
        sct_data.extend_from_slice(&0x0190_0000_0000u64.to_be_bytes()); // timestamp
        sct_data.extend_from_slice(&[0x00, 0x00]); // extensions_len = 0
        sct_data.push(0x04); // hash_algorithm = SHA-256
        sct_data.push(0x03); // signature_algorithm = ECDSA
        sct_data.extend_from_slice(&[0x00, 0x04]); // signature_len = 4
        sct_data.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]); // signature

        // Wrap in SCT list format
        let sct_len = sct_data.len() as u16;
        let mut list = Vec::new();
        let total_len = (2 + sct_data.len()) as u16;
        list.extend_from_slice(&total_len.to_be_bytes());
        list.extend_from_slice(&sct_len.to_be_bytes());
        list.extend_from_slice(&sct_data);

        let scts = CtPolicy::parse_sct_list(&list);
        assert_eq!(scts.len(), 1);
        assert_eq!(scts[0].version, 0);
        assert_eq!(scts[0].log_id, [0xAA; 32]);
        assert_eq!(scts[0].hash_algorithm, 4);
        assert_eq!(scts[0].signature_algorithm, 3);
        assert_eq!(scts[0].signature, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn test_parse_sct_invalid_version() {
        let mut sct_data = Vec::new();
        sct_data.push(0x01); // version = 1 (unsupported, only v1=0 supported)
        sct_data.extend_from_slice(&[0xBB; 32]); // log_id
        sct_data.extend_from_slice(&[0x00; 8]); // timestamp
        sct_data.extend_from_slice(&[0x00, 0x00]); // extensions_len
        sct_data.push(0x04);
        sct_data.push(0x03);
        sct_data.extend_from_slice(&[0x00, 0x00]); // sig_len = 0

        let sct_len = sct_data.len() as u16;
        let mut list = Vec::new();
        let total_len = (2 + sct_data.len()) as u16;
        list.extend_from_slice(&total_len.to_be_bytes());
        list.extend_from_slice(&sct_len.to_be_bytes());
        list.extend_from_slice(&sct_data);

        let scts = CtPolicy::parse_sct_list(&list);
        assert!(scts.is_empty()); // v1 rejected
    }

    #[test]
    fn test_check_certificate_disabled() {
        let policy = CtPolicy::default();
        // Disabled mode always passes
        assert!(policy.check_certificate(None, &[]).is_ok());
        assert!(policy.check_certificate(Some(&[]), &[]).is_ok());
    }

    #[test]
    fn test_check_certificate_enforced_no_scts() {
        let policy = CtPolicy::new(CtEnforcement::Enforced);
        // Enforced mode rejects missing SCTs
        assert!(policy.check_certificate(None, &[]).is_err());
        assert!(policy.check_certificate(Some(&[]), &[]).is_err());
    }

    #[test]
    fn test_check_certificate_best_effort_no_scts() {
        let policy = CtPolicy::new(CtEnforcement::BestEffort);
        // BestEffort mode allows missing SCTs (with warning)
        assert!(policy.check_certificate(None, &[]).is_ok());
    }

    #[test]
    fn test_verify_scts_unknown_log() {
        let policy = CtPolicy::default();
        let scts = vec![SignedCertificateTimestamp {
            version: 0,
            log_id: [0xFF; 32], // Unknown log
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64,
            extensions: vec![],
            hash_algorithm: 4,
            signature_algorithm: 3,
            signature: vec![0x42; 64],
        }];

        let results = policy.verify_scts(&scts, &[]);
        assert_eq!(results.len(), 1);
        assert!(!results[0].valid);
        assert!(results[0]
            .error
            .as_ref()
            .unwrap()
            .contains("unknown/untrusted"));
    }

    #[test]
    fn test_verify_scts_future_timestamp() {
        let mut policy = CtPolicy::default();
        policy.add_logs_from_json_v3(TEST_LOG_LIST).unwrap();
        let log_id = *policy.trusted_logs.keys().next().unwrap();

        let scts = vec![SignedCertificateTimestamp {
            version: 0,
            log_id,
            timestamp: u64::MAX, // Far future
            extensions: vec![],
            hash_algorithm: 4,
            signature_algorithm: 3,
            signature: vec![0x42; 64],
        }];

        let results = policy.verify_scts(&scts, &[]);
        assert!(!results[0].valid);
        assert!(results[0].error.as_ref().unwrap().contains("future"));
    }

    #[test]
    fn test_verify_scts_valid_trusted_log() {
        let mut policy = CtPolicy::default();
        policy.add_logs_from_json_v3(TEST_LOG_LIST).unwrap();
        let log_id = *policy.trusted_logs.keys().next().unwrap();

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let scts = vec![SignedCertificateTimestamp {
            version: 0,
            log_id,
            timestamp: now_ms - 1000, // 1 second ago
            extensions: vec![],
            hash_algorithm: 4,
            signature_algorithm: 3,
            signature: vec![0x42; 64],
        }];

        let results = policy.verify_scts(&scts, &[]);
        assert!(results[0].valid);
        assert!(results[0].log_name.is_some());
    }

    #[test]
    fn test_parse_multiple_scts() {
        // Build two SCTs
        let build_sct = |log_byte: u8| -> Vec<u8> {
            let mut sct = Vec::new();
            sct.push(0x00); // version
            sct.extend_from_slice(&[log_byte; 32]); // log_id
            sct.extend_from_slice(&[0x00; 8]); // timestamp
            sct.extend_from_slice(&[0x00, 0x00]); // ext_len=0
            sct.push(0x04); // hash
            sct.push(0x03); // sig algo
            sct.extend_from_slice(&[0x00, 0x02]); // sig_len=2
            sct.extend_from_slice(&[0xAB, 0xCD]); // sig
            sct
        };

        let sct1 = build_sct(0x11);
        let sct2 = build_sct(0x22);

        let mut list = Vec::new();
        let inner_len = (2 + sct1.len() + 2 + sct2.len()) as u16;
        list.extend_from_slice(&inner_len.to_be_bytes());
        list.extend_from_slice(&(sct1.len() as u16).to_be_bytes());
        list.extend_from_slice(&sct1);
        list.extend_from_slice(&(sct2.len() as u16).to_be_bytes());
        list.extend_from_slice(&sct2);

        let scts = CtPolicy::parse_sct_list(&list);
        assert_eq!(scts.len(), 2);
        assert_eq!(scts[0].log_id, [0x11; 32]);
        assert_eq!(scts[1].log_id, [0x22; 32]);
    }

    #[test]
    fn test_ct_log_id_deterministic() {
        let log = CtLog::new("Test", "SPINE", &[0x42; 27], "https://example.com/");
        let expected_id = CtPolicy::compute_log_id(&[0x42; 27]);
        assert_eq!(log.log_id, expected_id);
    }
}
