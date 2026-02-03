//! Sybil Resistance & Reputation System
//!
//! Implements stake-weighted voting and node reputation tracking
//! to prevent Sybil attacks on the consensus protocol.

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
// sha2 removed - using spine-crypto for hashing
use std::sync::Arc;
use uuid::Uuid;

/// Node ID type alias
pub type NodeId = Uuid;

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
}
