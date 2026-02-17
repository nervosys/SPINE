//! Sybil Resistance & Reputation System
//!
//! Implements stake-weighted voting and node reputation tracking
//! to prevent Sybil attacks on the consensus protocol.
//! Uses Argon2id memory-hard PoW to resist ASIC/GPU attacks.

use argon2::{Algorithm, Argon2, Params, Version};
use dashmap::DashMap;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use uuid::Uuid;

/// Node ID type alias
pub type NodeId = Uuid;

/// Memory-hard proof-of-work using Argon2id.
/// Resistant to ASIC/GPU acceleration (unlike CPU-bound SHA-256 PoW).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofOfWork {
    /// Node ID this proof is for
    pub node_id: NodeId,
    /// Random nonce that satisfies the difficulty target
    pub nonce: u64,
    /// Difficulty: number of leading zero bytes required in the hash
    pub difficulty: u32,
    /// The resulting hash (for quick verification)
    pub hash: [u8; 32],
}

impl ProofOfWork {
    /// Mine a proof-of-work for the given node ID and difficulty.
    /// Uses Argon2id with memory-hard parameters to resist GPU/ASIC attacks.
    /// `difficulty` = number of leading zero bytes required.
    pub fn mine(node_id: NodeId, difficulty: u32) -> Self {
        let mut rng = rand::thread_rng();
        loop {
            let nonce: u64 = rng.gen();
            let hash = Self::compute_hash(&node_id, nonce);
            if Self::meets_difficulty(&hash, difficulty) {
                return Self {
                    node_id,
                    nonce,
                    difficulty,
                    hash,
                };
            }
        }
    }

    /// Verify that the proof-of-work is valid.
    pub fn verify(&self) -> bool {
        let hash = Self::compute_hash(&self.node_id, self.nonce);
        hash == self.hash && Self::meets_difficulty(&hash, self.difficulty)
    }

    /// Compute the Argon2id hash for PoW.
    fn compute_hash(node_id: &NodeId, nonce: u64) -> [u8; 32] {
        // Construct input: node_id bytes || nonce bytes
        let mut input = Vec::with_capacity(24);
        input.extend_from_slice(node_id.as_bytes());
        input.extend_from_slice(&nonce.to_le_bytes());

        // Argon2id with memory-hard parameters:
        // m=4096 KiB, t=3 iterations, p=1 lane
        // This makes each hash attempt cost ~4 MiB of memory
        let params = Params::new(4096, 3, 1, Some(32)).expect("valid params");
        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

        // Use SHA-256 of input as salt (deterministic for verification)
        let mut hasher = Sha256::new();
        hasher.update(&input);
        let salt: [u8; 32] = hasher.finalize().into();

        let mut output = [0u8; 32];
        argon2
            .hash_password_into(&input, &salt[..16], &mut output)
            .expect("hash computation");
        output
    }

    /// Check if hash meets the required difficulty (leading zero bytes).
    fn meets_difficulty(hash: &[u8; 32], difficulty: u32) -> bool {
        let required = difficulty.min(32) as usize;
        hash[..required].iter().all(|&b| b == 0)
    }
}

/// Node reputation tracker for Sybil resistance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeReputation {
    pub node_id: NodeId,
    pub score: f32,
    pub successful_votes: u64,
    pub failed_votes: u64,
    pub proposals_accepted: u64,
    pub proposals_rejected: u64,
    pub joined_at: u64,
    pub stake: u64,
    pub pow_difficulty: u32,
}

impl NodeReputation {
    pub fn new(node_id: NodeId, stake: u64, pow_difficulty: u32) -> Self {
        Self {
            node_id,
            score: 0.5,
            successful_votes: 0,
            failed_votes: 0,
            proposals_accepted: 0,
            proposals_rejected: 0,
            joined_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            stake,
            pow_difficulty,
        }
    }

    pub fn record_vote(&mut self, aligned: bool) {
        if aligned {
            self.successful_votes += 1;
            self.score = (self.score + 0.01).min(1.0);
        } else {
            self.failed_votes += 1;
            self.score = (self.score - 0.02).max(0.0);
        }
    }

    pub fn voting_weight(&self) -> f32 {
        let age = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .saturating_sub(self.joined_at);
        let age_factor = (1.0 + (age as f32 / 86400.0).ln()).clamp(0.1, 3.0);
        let stake_factor = (self.stake as f32).sqrt();
        let pow_factor = 1.0 + (self.pow_difficulty as f32 * 0.1);
        self.score * stake_factor * age_factor * pow_factor
    }

    pub fn can_vote(&self) -> bool {
        self.score >= 0.1 && self.stake > 0
    }
}

/// Stake-weighted consensus
#[derive(Debug, Clone)]
pub struct StakeWeightedConsensus {
    reputations: Arc<DashMap<NodeId, NodeReputation>>,
    min_stake: u64,
    min_pow: u32,
    threshold: f32,
}

impl StakeWeightedConsensus {
    pub fn new(min_stake: u64, min_pow: u32, threshold: f32) -> Self {
        Self {
            reputations: Arc::new(DashMap::new()),
            min_stake,
            min_pow,
            threshold: threshold.clamp(0.5, 1.0),
        }
    }

    pub fn register_node(&self, id: NodeId, stake: u64, pow: u32) -> bool {
        if stake < self.min_stake || pow < self.min_pow { return false; }
        self.reputations.insert(id, NodeReputation::new(id, stake, pow));
        true
    }

    /// Register a node with verified Argon2id proof-of-work.
    /// This is the secure registration path that verifies the PoW before accepting.
    pub fn register_node_with_pow(&self, stake: u64, proof: &ProofOfWork) -> bool {
        if stake < self.min_stake { return false; }
        if proof.difficulty < self.min_pow { return false; }
        if !proof.verify() { return false; }
        self.reputations.insert(
            proof.node_id,
            NodeReputation::new(proof.node_id, stake, proof.difficulty),
        );
        true
    }

    pub fn check_consensus(&self, votes: &[(NodeId, bool, f32)]) -> (bool, f32) {
        let mut total = 0.0f32;
        let mut approve = 0.0f32;
        for (id, yes, conf) in votes {
            if let Some(r) = self.reputations.get(id) {
                if r.can_vote() {
                    let w = r.voting_weight() * conf;
                    total += w;
                    if *yes { approve += w; }
                }
            }
        }
        if total == 0.0 { return (false, 0.0); }
        let ratio = approve / total;
        (ratio >= self.threshold, ratio)
    }

    pub fn get_reputation(&self, id: &NodeId) -> Option<NodeReputation> {
        self.reputations.get(id).map(|r| r.clone())
    }

    pub fn slash_stake(&self, id: &NodeId, amount: u64) {
        if let Some(mut r) = self.reputations.get_mut(id) {
            r.stake = r.stake.saturating_sub(amount);
            r.score = (r.score - 0.1).max(0.0);
        }
    }

    pub fn record_vote_outcomes(&self, votes: &[(NodeId, bool)], result: bool) {
        for (id, vote) in votes {
            if let Some(mut r) = self.reputations.get_mut(id) {
                r.record_vote(*vote == result);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reputation_basic() {
        let id = Uuid::new_v4();
        let rep = NodeReputation::new(id, 100, 16);
        assert_eq!(rep.score, 0.5);
        assert!(rep.can_vote());
    }

    #[test]
    fn test_consensus_basic() {
        let c = StakeWeightedConsensus::new(10, 8, 0.67);
        let id = Uuid::new_v4();
        assert!(c.register_node(id, 100, 16));
        let (pass, _) = c.check_consensus(&[(id, true, 1.0)]);
        assert!(pass);
    }

    #[test]
    fn test_slashing() {
        let c = StakeWeightedConsensus::new(10, 8, 0.67);
        let id = Uuid::new_v4();
        c.register_node(id, 100, 16);
        c.slash_stake(&id, 50);
        let rep = c.get_reputation(&id).unwrap();
        assert_eq!(rep.stake, 50);
    }

    #[test]
    fn test_pow_mine_and_verify() {
        let id = Uuid::new_v4();
        let proof = ProofOfWork::mine(id, 1);
        assert!(proof.verify());
        assert_eq!(proof.node_id, id);
        assert_eq!(proof.difficulty, 1);
        assert_eq!(proof.hash[0], 0);
    }

    #[test]
    fn test_pow_invalid_nonce_rejected() {
        let id = Uuid::new_v4();
        let mut proof = ProofOfWork::mine(id, 1);
        proof.nonce = proof.nonce.wrapping_add(1);
        assert!(!proof.verify());
    }

    #[test]
    fn test_pow_wrong_node_rejected() {
        let id = Uuid::new_v4();
        let mut proof = ProofOfWork::mine(id, 1);
        proof.node_id = Uuid::new_v4();
        assert!(!proof.verify());
    }

    #[test]
    fn test_register_with_pow() {
        let c = StakeWeightedConsensus::new(10, 1, 0.67);
        let id = Uuid::new_v4();
        let proof = ProofOfWork::mine(id, 1);
        assert!(c.register_node_with_pow(100, &proof));
        assert!(c.get_reputation(&id).is_some());
    }

    #[test]
    fn test_register_with_pow_insufficient_difficulty() {
        let c = StakeWeightedConsensus::new(10, 2, 0.67);
        let id = Uuid::new_v4();
        let proof = ProofOfWork::mine(id, 1);
        assert!(!c.register_node_with_pow(100, &proof));
    }
}