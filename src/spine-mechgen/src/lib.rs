//! MechGen ABL ↔ SPINE bridge — the **type-safe join**.
//!
//! MechGen's `MechGen-parse --spine={profile,swarm,frame}` emits SPINE-protocol-
//! shaped JSON (see `MechGen/SPINE_COLLABORATION.md`). This crate deserializes
//! those envelopes into the *real* `spine-agentic` types, so the mapping is
//! **compile-checked** here rather than matched only by field name on the
//! MechGen side. It is intentionally tiny: a few envelope structs + conversions.
//!
//! The fact that an ABL agent's `capabilities` deserialize directly into
//! `Vec<spine_agentic::AgentCapability>` is the proof that the two capability
//! vocabularies line up.

use serde::Deserialize;
use spine_agentic::swe::{SweArtifact, SweArtifactKind};
use spine_agentic::{AgentCapability, AgentId, AgentProfile, Goal, SwarmTask, TrustLevel};
use uuid::Uuid;

/// Envelope emitted by `MechGen-parse --spine=profile`. Unknown fields
/// (`miras_variant`, …) are ignored by serde.
#[derive(Debug, Clone, Deserialize)]
pub struct AblAgentEnvelope {
    /// Agent name.
    pub name: String,
    /// Capabilities — deserialized straight into the SPINE enum (the join proof).
    pub capabilities: Vec<AgentCapability>,
    /// Trust level (defaults to `Unknown` if absent).
    #[serde(default)]
    pub trust_level: Option<TrustLevel>,
    /// MechGen approval-gated ops (enforced MechGen-side; kept for the receiver).
    #[serde(default)]
    pub requires_approval: Vec<String>,
}

impl AblAgentEnvelope {
    /// Parse the `--spine=profile` JSON.
    pub fn from_json(s: &str) -> Result<Self, String> {
        serde_json::from_str(s).map_err(|e| format!("bad agent envelope: {e}"))
    }
    /// Convert into a real `spine_agentic::AgentProfile`.
    pub fn into_profile(self) -> AgentProfile {
        AgentProfile::new(self.name)
            .with_capabilities(self.capabilities)
            .with_trust(self.trust_level.unwrap_or(TrustLevel::Unknown))
    }
}

/// Envelope emitted by `MechGen-parse --spine=swarm`.
#[derive(Debug, Clone, Deserialize)]
pub struct AblSwarmEnvelope {
    /// Swarm description (its name).
    pub description: String,
    /// Required capabilities — straight into the SPINE enum.
    pub required_capabilities: Vec<AgentCapability>,
    /// Minimum members.
    pub min_members: usize,
    /// Maximum members.
    pub max_members: usize,
    /// ABL topology (coordination metadata; not part of SwarmTask).
    #[serde(default)]
    pub topology: Option<String>,
    /// ABL consensus strategy (coordination metadata).
    #[serde(default)]
    pub consensus: Option<String>,
}

impl AblSwarmEnvelope {
    /// Parse the `--spine=swarm` JSON.
    pub fn from_json(s: &str) -> Result<Self, String> {
        serde_json::from_str(s).map_err(|e| format!("bad swarm envelope: {e}"))
    }
    /// Convert into a real `spine_agentic::SwarmTask` (goal = assemble agents
    /// with the required capabilities).
    pub fn into_task(self) -> SwarmTask {
        SwarmTask {
            id: Uuid::new_v4(),
            description: self.description,
            goal: Box::new(Goal::FindAgents { capabilities: self.required_capabilities.clone() }),
            min_members: self.min_members,
            max_members: self.max_members,
            required_capabilities: self.required_capabilities,
            deadline: None,
        }
    }
}

/// Envelope emitted by `MechGen-parse --spine=frame` — an ABL binary artifact.
#[derive(Debug, Clone, Deserialize)]
pub struct AblArtifactFrame {
    /// Byte length of the artifact.
    pub byte_len: usize,
    /// MechGen's FNV-1a-64 content digest (lowercase hex).
    pub content_digest: String,
    /// Always false — ABL load never executes code.
    pub exec: bool,
    /// Hex-encoded payload (inspection view; the real wire carries raw bytes).
    pub payload_hex: String,
}

/// FNV-1a 64 — must match MechGen's `spine_bridge::content_digest`.
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn from_hex(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err("odd-length hex".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| format!("bad hex: {e}")))
        .collect()
}

impl AblArtifactFrame {
    /// Parse the `--spine=frame` JSON.
    pub fn from_json(s: &str) -> Result<Self, String> {
        serde_json::from_str(s).map_err(|e| format!("bad frame: {e}"))
    }

    /// Decode + **cross-validate** the frame (recompute the FNV digest over the
    /// decoded bytes and check it matches), then produce a content-addressed
    /// (SHA-256) `SweArtifact` for the SPINE artifact store. Errors on a digest
    /// mismatch or a non-`false` `exec` flag.
    pub fn into_artifact(self, producer: AgentId) -> Result<SweArtifact, String> {
        if self.exec {
            return Err("frame.exec must be false (ABL load is no-exec)".into());
        }
        let bytes = from_hex(&self.payload_hex)?;
        if bytes.len() != self.byte_len {
            return Err(format!("byte_len {} != decoded {}", self.byte_len, bytes.len()));
        }
        let got = format!("{:016x}", fnv1a64(&bytes));
        if got != self.content_digest {
            return Err(format!("digest mismatch: frame={} computed={}", self.content_digest, got));
        }
        Ok(SweArtifact::new(&bytes, SweArtifactKind::Other("abl-artifact".into()), producer))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The EXACT JSON shapes `MechGen-parse --spine=profile/swarm` emit.
    #[test]
    fn agent_envelope_maps_to_profile() {
        let json = r#"{"name":"Builder","capabilities":["AgentCommunication","SwarmParticipation","ContentExtraction","CodeExecution"],"trust_level":"Verified","miras_variant":"Titans","requires_approval":["write_files"]}"#;
        let env = AblAgentEnvelope::from_json(json).expect("parse");
        // capabilities deserialized into the real SPINE enum — the join proof.
        assert!(env.capabilities.contains(&AgentCapability::CodeExecution));
        assert_eq!(env.requires_approval, vec!["write_files".to_string()]);
        let profile = env.into_profile();
        assert_eq!(profile.name, "Builder");
        assert_eq!(profile.trust_level, TrustLevel::Verified);
        assert!(profile.capabilities.contains(&AgentCapability::SwarmParticipation));
    }

    #[test]
    fn custom_capability_round_trips_through_the_enum() {
        let json = r#"{"name":"Reviewer","capabilities":[{"Custom":"review_pr"}]}"#;
        let env = AblAgentEnvelope::from_json(json).expect("parse");
        assert_eq!(env.capabilities, vec![AgentCapability::Custom("review_pr".into())]);
    }

    #[test]
    fn swarm_envelope_maps_to_task() {
        let json = r#"{"description":"Reviewers","required_capabilities":[{"Custom":"Reviewer"}],"min_members":5,"max_members":5,"topology":"ring","consensus":"quorum"}"#;
        let env = AblSwarmEnvelope::from_json(json).expect("parse");
        let task = env.into_task();
        assert_eq!(task.description, "Reviewers");
        assert_eq!(task.min_members, 5);
        assert!(matches!(*task.goal, Goal::FindAgents { .. }));
        assert_eq!(task.required_capabilities, vec![AgentCapability::Custom("Reviewer".into())]);
    }

    #[test]
    fn frame_cross_validates_and_yields_artifact() {
        // Build a frame exactly as MechGen's --spine=frame does.
        let bytes = b"ABL1\x02\x00demo-net";
        let digest = format!("{:016x}", fnv1a64(bytes));
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let json = format!(
            r#"{{"kind":"abl-artifact","byte_len":{},"content_digest":"{}","exec":false,"signed":false,"payload_hex":"{}"}}"#,
            bytes.len(), digest, hex
        );
        let frame = AblArtifactFrame::from_json(&json).expect("parse");
        let art = frame.into_artifact(AgentId::new()).expect("valid frame");
        assert_eq!(art.size, bytes.len());
        assert!(!art.content_hash.is_empty(), "sha-256 content address assigned");
    }

    #[test]
    fn frame_rejects_digest_mismatch() {
        let json = r#"{"kind":"abl-artifact","byte_len":3,"content_digest":"deadbeefdeadbeef","exec":false,"signed":false,"payload_hex":"414243"}"#;
        let frame = AblArtifactFrame::from_json(json).unwrap();
        assert!(frame.into_artifact(AgentId::new()).is_err(), "tampered digest must be rejected");
    }
}
