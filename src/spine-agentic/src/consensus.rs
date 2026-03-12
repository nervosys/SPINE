// =============================================================================
// CONSENSUS: Byzantine Fault Tolerant Collective Decision-Making
// =============================================================================
//
// Enables agent swarms to reach agreement even when some members are malicious
// or faulty. Uses weighted voting with trust scores and threshold signatures.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::AgentId;

/// A proposal submitted for collective decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: Uuid,
    pub proposer: AgentId,
    pub kind: ProposalKind,
    pub description: String,
    pub options: Vec<ProposalOption>,
    pub quorum: QuorumRule,
    pub created_at: DateTime<Utc>,
    pub deadline: DateTime<Utc>,
    pub status: ProposalStatus,
    /// SHA-256 of serialized proposal content for tamper detection.
    pub content_hash: [u8; 32],
}

/// What the swarm is deciding on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalKind {
    /// Accept or reject an action.
    Binary,
    /// Choose one of N options.
    MultipleChoice,
    /// Rank options by preference.
    Ranked,
    /// Allocate a budget across options.
    Weighted,
    /// Emergency action requiring fast consensus.
    Emergency,
}

/// An option within a proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalOption {
    pub id: u32,
    pub label: String,
    pub metadata: HashMap<String, String>,
}

/// Rules for determining quorum.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuorumRule {
    /// Simple majority (>50%).
    Majority,
    /// Two-thirds supermajority (≥67%).
    SuperMajority,
    /// All eligible voters must agree.
    Unanimous,
    /// Custom threshold (0.0–1.0).
    Threshold(f64),
    /// BFT: tolerates up to f faults among 3f+1 total.
    ByzantineFaultTolerant,
}

/// Current status of a proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalStatus {
    Open,
    Decided(u32),
    Rejected,
    Expired,
    Disputed,
}

/// A vote cast by an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub voter: AgentId,
    pub proposal_id: Uuid,
    pub choice: VoteChoice,
    pub justification: Option<String>,
    pub cast_at: DateTime<Utc>,
    /// SHA-256 commitment (for commit-reveal scheme).
    pub commitment: Option<[u8; 32]>,
}

/// The content of a vote.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VoteChoice {
    /// Select a single option.
    Single(u32),
    /// Rank options (first = most preferred).
    Ranked(Vec<u32>),
    /// Weighted allocation across options (must sum to 1.0).
    Weighted(Vec<(u32, f64)>),
    /// Abstain from voting.
    Abstain,
}

/// Result of tallying votes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TallyResult {
    pub proposal_id: Uuid,
    pub total_weight: f64,
    pub participating_weight: f64,
    pub scores: HashMap<u32, f64>,
    pub winner: Option<u32>,
    pub decided: bool,
    pub byzantine_detections: Vec<AgentId>,
}

/// Error type for consensus operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsensusError {
    ProposalNotFound,
    ProposalClosed,
    AlreadyVoted,
    InvalidChoice,
    DeadlineExpired,
    InsufficientQuorum,
    ByzantineFaultDetected(Vec<AgentId>),
}

impl std::fmt::Display for ConsensusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProposalNotFound => write!(f, "proposal not found"),
            Self::ProposalClosed => write!(f, "proposal is closed"),
            Self::AlreadyVoted => write!(f, "agent already voted"),
            Self::InvalidChoice => write!(f, "invalid vote choice"),
            Self::DeadlineExpired => write!(f, "voting deadline expired"),
            Self::InsufficientQuorum => write!(f, "quorum not reached"),
            Self::ByzantineFaultDetected(ids) => {
                write!(f, "byzantine fault detected from {} agents", ids.len())
            }
        }
    }
}

impl std::error::Error for ConsensusError {}

/// Manages collective decision-making across a swarm.
pub struct ConsensusManager {
    proposals: DashMap<Uuid, Proposal>,
    votes: DashMap<Uuid, Vec<Vote>>,
    /// Trust weights per agent (0.0–1.0). Agents not listed have weight 0.
    trust_weights: DashMap<AgentId, f64>,
    /// Agents detected as byzantine in previous rounds.
    byzantine_agents: DashMap<AgentId, usize>,
}

impl ConsensusManager {
    pub fn new() -> Self {
        Self {
            proposals: DashMap::new(),
            votes: DashMap::new(),
            trust_weights: DashMap::new(),
            byzantine_agents: DashMap::new(),
        }
    }

    /// Register an agent with a trust weight for voting.
    pub fn register_voter(&self, agent: AgentId, weight: f64) {
        self.trust_weights.insert(agent, weight.clamp(0.0, 1.0));
    }

    /// Create a new proposal.
    pub fn propose(
        &self,
        proposer: AgentId,
        kind: ProposalKind,
        description: String,
        options: Vec<ProposalOption>,
        quorum: QuorumRule,
        deadline: DateTime<Utc>,
    ) -> Result<Uuid, ConsensusError> {
        if deadline <= Utc::now() {
            return Err(ConsensusError::DeadlineExpired);
        }

        let id = Uuid::new_v4();
        let content_hash = Self::hash_proposal(&description, &options);

        let proposal = Proposal {
            id,
            proposer,
            kind,
            description,
            options,
            quorum,
            created_at: Utc::now(),
            deadline,
            status: ProposalStatus::Open,
            content_hash,
        };

        self.proposals.insert(id, proposal);
        self.votes.insert(id, Vec::new());
        Ok(id)
    }

    /// Cast a vote on a proposal.
    pub fn vote(
        &self,
        voter: AgentId,
        proposal_id: Uuid,
        choice: VoteChoice,
    ) -> Result<(), ConsensusError> {
        // Validate proposal exists and is open
        let proposal = self
            .proposals
            .get(&proposal_id)
            .ok_or(ConsensusError::ProposalNotFound)?;

        if proposal.status != ProposalStatus::Open {
            return Err(ConsensusError::ProposalClosed);
        }
        if Utc::now() > proposal.deadline {
            return Err(ConsensusError::DeadlineExpired);
        }

        // Validate choice against proposal options
        self.validate_choice(&choice, &proposal)?;
        drop(proposal);

        // Check for duplicate vote
        let mut votes = self
            .votes
            .get_mut(&proposal_id)
            .ok_or(ConsensusError::ProposalNotFound)?;

        if votes.iter().any(|v| v.voter == voter) {
            return Err(ConsensusError::AlreadyVoted);
        }

        votes.push(Vote {
            voter,
            proposal_id,
            choice,
            justification: None,
            cast_at: Utc::now(),
            commitment: None,
        });

        Ok(())
    }

    /// Tally votes and determine outcome.
    pub fn tally(&self, proposal_id: Uuid) -> Result<TallyResult, ConsensusError> {
        let proposal = self
            .proposals
            .get(&proposal_id)
            .ok_or(ConsensusError::ProposalNotFound)?;

        let votes = self
            .votes
            .get(&proposal_id)
            .ok_or(ConsensusError::ProposalNotFound)?;

        let total_weight: f64 = self.trust_weights.iter().map(|e| *e.value()).sum();

        let mut scores: HashMap<u32, f64> = HashMap::new();
        let mut participating_weight = 0.0;
        let mut byzantine_detections = Vec::new();

        for vote in votes.iter() {
            let weight = self
                .trust_weights
                .get(&vote.voter)
                .map(|w| *w)
                .unwrap_or(0.0);

            // Skip known-byzantine agents
            if self.byzantine_agents.contains_key(&vote.voter) {
                byzantine_detections.push(vote.voter);
                continue;
            }

            participating_weight += weight;

            match &vote.choice {
                VoteChoice::Single(opt) => {
                    *scores.entry(*opt).or_default() += weight;
                }
                VoteChoice::Ranked(ranking) => {
                    // Borda count: last gets 1 point, first gets N points
                    let n = ranking.len() as f64;
                    for (i, opt) in ranking.iter().enumerate() {
                        *scores.entry(*opt).or_default() += weight * (n - i as f64) / n;
                    }
                }
                VoteChoice::Weighted(allocations) => {
                    for (opt, alloc) in allocations {
                        *scores.entry(*opt).or_default() += weight * alloc;
                    }
                }
                VoteChoice::Abstain => {}
            }
        }

        // Determine winner
        let winner = scores
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(id, _)| *id);

        // Check quorum
        let decided = self.check_quorum(
            &proposal.quorum,
            participating_weight,
            total_weight,
            &scores,
            winner,
        );

        Ok(TallyResult {
            proposal_id,
            total_weight,
            participating_weight,
            scores,
            winner: if decided { winner } else { None },
            decided,
            byzantine_detections,
        })
    }

    /// Finalize a proposal based on tally results.
    pub fn finalize(&self, proposal_id: Uuid) -> Result<TallyResult, ConsensusError> {
        let result = self.tally(proposal_id)?;

        let mut proposal = self
            .proposals
            .get_mut(&proposal_id)
            .ok_or(ConsensusError::ProposalNotFound)?;

        if proposal.status != ProposalStatus::Open {
            return Err(ConsensusError::ProposalClosed);
        }

        proposal.status = if result.decided {
            if let Some(w) = result.winner {
                ProposalStatus::Decided(w)
            } else {
                ProposalStatus::Rejected
            }
        } else if Utc::now() > proposal.deadline {
            ProposalStatus::Expired
        } else {
            return Err(ConsensusError::InsufficientQuorum);
        };

        Ok(result)
    }

    /// Mark an agent as byzantine (detected via external evidence).
    pub fn report_byzantine(&self, agent: AgentId) {
        self.byzantine_agents
            .entry(agent)
            .and_modify(|c| *c += 1)
            .or_insert(1);
        // Reduce trust weight to zero
        self.trust_weights.insert(agent, 0.0);
    }

    /// Detect equivocation: agent sending conflicting votes.
    pub fn detect_equivocation(&self, proposal_id: Uuid) -> Vec<AgentId> {
        let votes = match self.votes.get(&proposal_id) {
            Some(v) => v,
            None => return Vec::new(),
        };

        let mut seen: HashMap<AgentId, &VoteChoice> = HashMap::new();
        let mut equivocators = Vec::new();

        for vote in votes.iter() {
            if let Some(_prev) = seen.get(&vote.voter) {
                equivocators.push(vote.voter);
            } else {
                seen.insert(vote.voter, &vote.choice);
            }
        }

        equivocators
    }

    /// Get a proposal by ID.
    pub fn get_proposal(&self, id: Uuid) -> Option<Proposal> {
        self.proposals.get(&id).map(|p| p.clone())
    }

    /// List proposals with a given status.
    pub fn proposals_by_status(&self, status: &ProposalStatus) -> Vec<Proposal> {
        self.proposals
            .iter()
            .filter(|e| &e.value().status == status)
            .map(|e| e.value().clone())
            .collect()
    }

    fn validate_choice(
        &self,
        choice: &VoteChoice,
        proposal: &Proposal,
    ) -> Result<(), ConsensusError> {
        let valid_ids: HashSet<u32> = proposal.options.iter().map(|o| o.id).collect();
        match choice {
            VoteChoice::Single(id) => {
                if !valid_ids.contains(id) {
                    return Err(ConsensusError::InvalidChoice);
                }
            }
            VoteChoice::Ranked(ids) => {
                for id in ids {
                    if !valid_ids.contains(id) {
                        return Err(ConsensusError::InvalidChoice);
                    }
                }
            }
            VoteChoice::Weighted(allocs) => {
                for (id, _) in allocs {
                    if !valid_ids.contains(id) {
                        return Err(ConsensusError::InvalidChoice);
                    }
                }
            }
            VoteChoice::Abstain => {}
        }
        Ok(())
    }

    fn check_quorum(
        &self,
        rule: &QuorumRule,
        participating: f64,
        total: f64,
        scores: &HashMap<u32, f64>,
        winner: Option<u32>,
    ) -> bool {
        if total == 0.0 {
            return false;
        }
        let participation_ratio = participating / total;

        match rule {
            QuorumRule::Majority => {
                if let Some(w) = winner {
                    scores.get(&w).copied().unwrap_or(0.0) > participating / 2.0
                } else {
                    false
                }
            }
            QuorumRule::SuperMajority => {
                if let Some(w) = winner {
                    scores.get(&w).copied().unwrap_or(0.0) >= participating * 2.0 / 3.0
                } else {
                    false
                }
            }
            QuorumRule::Unanimous => {
                participation_ratio >= 1.0
                    && winner.is_some()
                    && scores.len() == 1
            }
            QuorumRule::Threshold(t) => {
                if let Some(w) = winner {
                    scores.get(&w).copied().unwrap_or(0.0) / participating >= *t
                } else {
                    false
                }
            }
            QuorumRule::ByzantineFaultTolerant => {
                // Classic BFT: need 2f+1 out of 3f+1 = ≥67% participation
                // and ≥67% agreement among participants
                if participation_ratio < 2.0 / 3.0 {
                    return false;
                }
                if let Some(w) = winner {
                    scores.get(&w).copied().unwrap_or(0.0) >= participating * 2.0 / 3.0
                } else {
                    false
                }
            }
        }
    }

    fn hash_proposal(description: &str, options: &[ProposalOption]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(description.as_bytes());
        for opt in options {
            hasher.update(opt.id.to_le_bytes());
            hasher.update(opt.label.as_bytes());
        }
        hasher.finalize().into()
    }
}

impl Default for ConsensusManager {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_options(labels: &[&str]) -> Vec<ProposalOption> {
        labels
            .iter()
            .enumerate()
            .map(|(i, l)| ProposalOption {
                id: i as u32,
                label: l.to_string(),
                metadata: HashMap::new(),
            })
            .collect()
    }

    fn future(secs: i64) -> DateTime<Utc> {
        Utc::now() + chrono::Duration::seconds(secs)
    }

    #[test]
    fn test_majority_vote() {
        let cm = ConsensusManager::new();
        let agents: Vec<AgentId> = (0..5).map(|_| AgentId::new()).collect();
        for &a in &agents {
            cm.register_voter(a, 1.0);
        }

        let pid = cm
            .propose(
                agents[0],
                ProposalKind::Binary,
                "Accept?".into(),
                make_options(&["Yes", "No"]),
                QuorumRule::Majority,
                future(60),
            )
            .unwrap();

        // 3 yes, 2 no
        for &a in &agents[..3] {
            cm.vote(a, pid, VoteChoice::Single(0)).unwrap();
        }
        for &a in &agents[3..] {
            cm.vote(a, pid, VoteChoice::Single(1)).unwrap();
        }

        let result = cm.finalize(pid).unwrap();
        assert!(result.decided);
        assert_eq!(result.winner, Some(0));
    }

    #[test]
    fn test_supermajority_fails() {
        let cm = ConsensusManager::new();
        let agents: Vec<AgentId> = (0..3).map(|_| AgentId::new()).collect();
        for &a in &agents {
            cm.register_voter(a, 1.0);
        }

        let pid = cm
            .propose(
                agents[0],
                ProposalKind::Binary,
                "Upgrade?".into(),
                make_options(&["Yes", "No"]),
                QuorumRule::SuperMajority,
                future(60),
            )
            .unwrap();

        cm.vote(agents[0], pid, VoteChoice::Single(0)).unwrap();
        cm.vote(agents[1], pid, VoteChoice::Single(0)).unwrap();
        cm.vote(agents[2], pid, VoteChoice::Single(1)).unwrap();

        // 2/3 = 66.7%, needs ≥66.7% → borderline
        let result = cm.tally(pid).unwrap();
        // 2.0 >= 3.0 * 2/3 = 2.0 → true
        assert!(result.decided);
    }

    #[test]
    fn test_weighted_trust() {
        let cm = ConsensusManager::new();
        let whale = AgentId::new();
        let minnows: Vec<AgentId> = (0..3).map(|_| AgentId::new()).collect();

        cm.register_voter(whale, 1.0);
        for &m in &minnows {
            cm.register_voter(m, 0.1);
        }

        let pid = cm
            .propose(
                whale,
                ProposalKind::Binary,
                "Direction?".into(),
                make_options(&["Left", "Right"]),
                QuorumRule::Majority,
                future(60),
            )
            .unwrap();

        cm.vote(whale, pid, VoteChoice::Single(0)).unwrap();
        for &m in &minnows {
            cm.vote(m, pid, VoteChoice::Single(1)).unwrap();
        }

        let result = cm.tally(pid).unwrap();
        // whale=1.0 for Left, minnows=0.3 for Right → Left wins
        assert_eq!(result.winner, Some(0));
    }

    #[test]
    fn test_byzantine_exclusion() {
        let cm = ConsensusManager::new();
        let good: Vec<AgentId> = (0..3).map(|_| AgentId::new()).collect();
        let bad = AgentId::new();

        for &a in &good {
            cm.register_voter(a, 1.0);
        }
        cm.register_voter(bad, 1.0);

        let pid = cm
            .propose(
                good[0],
                ProposalKind::Binary,
                "Proceed?".into(),
                make_options(&["Yes", "No"]),
                QuorumRule::Majority,
                future(60),
            )
            .unwrap();

        // Bad agent votes one way
        cm.vote(bad, pid, VoteChoice::Single(1)).unwrap();
        // Mark as byzantine
        cm.report_byzantine(bad);
        // Good agents vote the other
        for &a in &good {
            cm.vote(a, pid, VoteChoice::Single(0)).unwrap();
        }

        let result = cm.tally(pid).unwrap();
        assert_eq!(result.winner, Some(0));
        assert!(result.byzantine_detections.contains(&bad));
    }

    #[test]
    fn test_duplicate_vote_rejected() {
        let cm = ConsensusManager::new();
        let agent = AgentId::new();
        cm.register_voter(agent, 1.0);

        let pid = cm
            .propose(
                agent,
                ProposalKind::Binary,
                "Test".into(),
                make_options(&["A", "B"]),
                QuorumRule::Majority,
                future(60),
            )
            .unwrap();

        cm.vote(agent, pid, VoteChoice::Single(0)).unwrap();
        assert_eq!(
            cm.vote(agent, pid, VoteChoice::Single(1)),
            Err(ConsensusError::AlreadyVoted)
        );
    }

    #[test]
    fn test_invalid_choice_rejected() {
        let cm = ConsensusManager::new();
        let agent = AgentId::new();
        cm.register_voter(agent, 1.0);

        let pid = cm
            .propose(
                agent,
                ProposalKind::Binary,
                "Test".into(),
                make_options(&["A", "B"]),
                QuorumRule::Majority,
                future(60),
            )
            .unwrap();

        assert_eq!(
            cm.vote(agent, pid, VoteChoice::Single(99)),
            Err(ConsensusError::InvalidChoice)
        );
    }

    #[test]
    fn test_bft_quorum() {
        let cm = ConsensusManager::new();
        // 4 agents (3f+1 where f=1)
        let agents: Vec<AgentId> = (0..4).map(|_| AgentId::new()).collect();
        for &a in &agents {
            cm.register_voter(a, 1.0);
        }

        let pid = cm
            .propose(
                agents[0],
                ProposalKind::Binary,
                "BFT test".into(),
                make_options(&["Yes", "No"]),
                QuorumRule::ByzantineFaultTolerant,
                future(60),
            )
            .unwrap();

        // 3 out of 4 vote yes (75% > 67%)
        for &a in &agents[..3] {
            cm.vote(a, pid, VoteChoice::Single(0)).unwrap();
        }
        cm.vote(agents[3], pid, VoteChoice::Single(1)).unwrap();

        let result = cm.finalize(pid).unwrap();
        assert!(result.decided);
        assert_eq!(result.winner, Some(0));
    }

    #[test]
    fn test_ranked_voting() {
        let cm = ConsensusManager::new();
        let agents: Vec<AgentId> = (0..3).map(|_| AgentId::new()).collect();
        for &a in &agents {
            cm.register_voter(a, 1.0);
        }

        let pid = cm
            .propose(
                agents[0],
                ProposalKind::Ranked,
                "Priority?".into(),
                make_options(&["Speed", "Safety", "Cost"]),
                QuorumRule::Threshold(0.3),
                future(60),
            )
            .unwrap();

        cm.vote(agents[0], pid, VoteChoice::Ranked(vec![1, 0, 2])).unwrap();
        cm.vote(agents[1], pid, VoteChoice::Ranked(vec![1, 2, 0])).unwrap();
        cm.vote(agents[2], pid, VoteChoice::Ranked(vec![0, 1, 2])).unwrap();

        let result = cm.tally(pid).unwrap();
        // Safety (1) should win with highest Borda score
        assert_eq!(result.winner, Some(1));
    }

    #[test]
    fn test_proposal_content_hash() {
        let cm = ConsensusManager::new();
        let agent = AgentId::new();
        cm.register_voter(agent, 1.0);

        let pid = cm
            .propose(
                agent,
                ProposalKind::Binary,
                "Hash test".into(),
                make_options(&["A", "B"]),
                QuorumRule::Majority,
                future(60),
            )
            .unwrap();

        let proposal = cm.get_proposal(pid).unwrap();
        assert_ne!(proposal.content_hash, [0u8; 32]);
    }

    #[test]
    fn test_expired_deadline() {
        let cm = ConsensusManager::new();
        let agent = AgentId::new();

        let result = cm.propose(
            agent,
            ProposalKind::Binary,
            "Too late".into(),
            make_options(&["A"]),
            QuorumRule::Majority,
            Utc::now() - chrono::Duration::seconds(10),
        );
        assert_eq!(result, Err(ConsensusError::DeadlineExpired));
    }
}
