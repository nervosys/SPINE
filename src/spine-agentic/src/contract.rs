// =============================================================================
// SPINE Agent-to-Agent Contract Negotiation
// =============================================================================
//
// Structured contracts between agents with SLA enforcement, resource budgets,
// deadlines, and lifecycle management (propose → accept → execute → settle).
//
// Builds on the existing NegotiationProtocol but adds formal contract
// semantics with verifiable obligations.
//
// =============================================================================

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use uuid::Uuid;

use crate::AgentId;

// =============================================================================
// CONTRACT DEFINITIONS
// =============================================================================

/// Unique identifier for a contract.
pub type ContractId = Uuid;

/// A formal contract between two or more agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contract {
    pub id: ContractId,
    pub name: String,
    /// The agent proposing the contract.
    pub proposer: AgentId,
    /// The agent(s) who must accept the contract.
    pub counterparties: Vec<AgentId>,
    /// What the proposer will provide.
    pub proposer_obligations: Vec<Obligation>,
    /// What each counterparty must provide.
    pub counterparty_obligations: Vec<Obligation>,
    /// Service level agreement terms.
    pub sla: ContractSla,
    /// Resource budget allocated to this contract.
    pub resource_budget: ResourceBudget,
    /// Contract lifecycle status.
    pub status: ContractStatus,
    /// Which counterparties have accepted.
    pub acceptances: Vec<AgentId>,
    /// When the contract was proposed.
    pub proposed_at: DateTime<Utc>,
    /// When the contract became active (all parties accepted).
    pub activated_at: Option<DateTime<Utc>>,
    /// When the contract was settled (completed or terminated).
    pub settled_at: Option<DateTime<Utc>>,
    /// Cryptographic hash of the contract terms for integrity verification.
    pub terms_hash: String,
    /// Dispute records, if any.
    pub disputes: Vec<Dispute>,
}

/// The lifecycle status of a contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContractStatus {
    /// Contract has been proposed but not yet accepted by all parties.
    Proposed,
    /// All parties have accepted; contract is being executed.
    Active,
    /// Contract has been fulfilled by all parties.
    Fulfilled,
    /// Contract was breached by one or more parties.
    Breached,
    /// Contract was cancelled before activation.
    Cancelled,
    /// Contract has been settled (post-execution resolution).
    Settled,
    /// Contract is in dispute.
    Disputed,
    /// Contract expired before completion.
    Expired,
}

/// An obligation that a party must fulfill.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Obligation {
    pub id: Uuid,
    pub description: String,
    /// What kind of deliverable.
    pub obligation_type: ObligationType,
    /// Deadline for this specific obligation.
    pub deadline: Option<DateTime<Utc>>,
    /// Whether this obligation has been fulfilled.
    pub fulfilled: bool,
    /// Evidence of fulfillment (e.g., task result hash).
    pub evidence: Option<String>,
}

/// Types of obligations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ObligationType {
    /// Execute a specific task and return results.
    TaskExecution {
        task_description: String,
        required_capabilities: Vec<String>,
    },
    /// Provide data/knowledge.
    DataDelivery {
        data_type: String,
        min_quality: f64,
    },
    /// Provide ongoing service for a duration.
    ServiceProvision {
        service_type: String,
        duration_secs: u64,
    },
    /// Pay credits/tokens.
    Payment { amount: f64, currency: String },
    /// Share knowledge or capabilities.
    KnowledgeSharing { topics: Vec<String> },
    /// Custom obligation.
    Custom { key: String, value: serde_json::Value },
}

// =============================================================================
// SLA & RESOURCE BUDGET
// =============================================================================

/// Service level agreement for a contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractSla {
    /// Maximum response time in milliseconds.
    pub max_response_ms: Option<u64>,
    /// Minimum uptime/availability percentage (0.0–1.0).
    pub min_availability: Option<f64>,
    /// Maximum error rate (0.0–1.0).
    pub max_error_rate: Option<f64>,
    /// Minimum quality score for deliverables (0.0–1.0).
    pub min_quality: Option<f64>,
    /// Overall contract deadline.
    pub deadline: Option<DateTime<Utc>>,
    /// Penalty for SLA breach (in credits/tokens).
    pub breach_penalty: f64,
}

impl Default for ContractSla {
    fn default() -> Self {
        Self {
            max_response_ms: None,
            min_availability: None,
            max_error_rate: None,
            min_quality: None,
            deadline: None,
            breach_penalty: 0.0,
        }
    }
}

/// Resource budget for contract execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceBudget {
    /// Maximum compute units.
    pub max_compute: Option<f64>,
    /// Maximum memory in bytes.
    pub max_memory_bytes: Option<u64>,
    /// Maximum bandwidth in bytes.
    pub max_bandwidth_bytes: Option<u64>,
    /// Maximum API calls.
    pub max_api_calls: Option<u64>,
    /// Total credit budget.
    pub credit_budget: f64,
    /// Consumed resources so far.
    pub consumed: ResourceUsage,
}

impl Default for ResourceBudget {
    fn default() -> Self {
        Self {
            max_compute: None,
            max_memory_bytes: None,
            max_bandwidth_bytes: None,
            max_api_calls: None,
            credit_budget: 0.0,
            consumed: ResourceUsage::default(),
        }
    }
}

/// Tracked resource consumption.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResourceUsage {
    pub compute_used: f64,
    pub memory_bytes_used: u64,
    pub bandwidth_bytes_used: u64,
    pub api_calls_used: u64,
    pub credits_used: f64,
}

impl ResourceBudget {
    /// Check if any budget limits have been exceeded.
    pub fn is_exceeded(&self) -> bool {
        if let Some(max) = self.max_compute {
            if self.consumed.compute_used > max {
                return true;
            }
        }
        if let Some(max) = self.max_memory_bytes {
            if self.consumed.memory_bytes_used > max {
                return true;
            }
        }
        if let Some(max) = self.max_api_calls {
            if self.consumed.api_calls_used > max {
                return true;
            }
        }
        self.consumed.credits_used > self.credit_budget && self.credit_budget > 0.0
    }
}

// =============================================================================
// DISPUTES
// =============================================================================

/// A dispute raised by a party about a contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dispute {
    pub id: Uuid,
    pub raised_by: AgentId,
    pub reason: String,
    pub evidence: Option<String>,
    pub raised_at: DateTime<Utc>,
    pub resolved: bool,
    pub resolution: Option<DisputeResolution>,
}

/// Resolution of a dispute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DisputeResolution {
    /// Proposer was right, contract continues.
    ProposerFavored { explanation: String },
    /// Counterparty was right, contract adjusted.
    CounterpartyFavored { explanation: String },
    /// Compromise reached.
    Compromise { adjustment: String },
    /// Contract terminated.
    ContractTerminated { reason: String },
}

// =============================================================================
// CONTRACT MANAGER
// =============================================================================

/// Manages the lifecycle of contracts for an agent.
pub struct ContractManager {
    agent_id: AgentId,
    contracts: DashMap<ContractId, Contract>,
    /// Index: contracts where this agent is the proposer.
    proposed: DashMap<AgentId, Vec<ContractId>>,
    /// Index: contracts where this agent is a counterparty.
    received: DashMap<AgentId, Vec<ContractId>>,
}

impl ContractManager {
    pub fn new(agent_id: AgentId) -> Self {
        Self {
            agent_id,
            contracts: DashMap::new(),
            proposed: DashMap::new(),
            received: DashMap::new(),
        }
    }

    /// Propose a new contract.
    pub fn propose(
        &self,
        name: impl Into<String>,
        counterparties: Vec<AgentId>,
        proposer_obligations: Vec<Obligation>,
        counterparty_obligations: Vec<Obligation>,
        sla: ContractSla,
        resource_budget: ResourceBudget,
    ) -> Contract {
        let name = name.into();
        let terms_hash = compute_terms_hash(
            &name,
            &self.agent_id,
            &counterparties,
            &proposer_obligations,
            &counterparty_obligations,
        );

        let contract = Contract {
            id: Uuid::new_v4(),
            name,
            proposer: self.agent_id,
            counterparties: counterparties.clone(),
            proposer_obligations,
            counterparty_obligations,
            sla,
            resource_budget,
            status: ContractStatus::Proposed,
            acceptances: Vec::new(),
            proposed_at: Utc::now(),
            activated_at: None,
            settled_at: None,
            terms_hash,
            disputes: Vec::new(),
        };

        self.contracts.insert(contract.id, contract.clone());
        self.proposed
            .entry(self.agent_id)
            .or_default()
            .push(contract.id);

        for cp in &counterparties {
            self.received.entry(*cp).or_default().push(contract.id);
        }

        contract
    }

    /// Accept a contract (called by a counterparty).
    pub fn accept(
        &self,
        contract_id: ContractId,
        acceptor: AgentId,
    ) -> Result<ContractStatus, ContractError> {
        let mut contract = self
            .contracts
            .get_mut(&contract_id)
            .ok_or(ContractError::NotFound(contract_id))?;

        if contract.status != ContractStatus::Proposed {
            return Err(ContractError::InvalidStatus {
                expected: ContractStatus::Proposed,
                actual: contract.status,
            });
        }

        if !contract.counterparties.contains(&acceptor) {
            return Err(ContractError::NotAParty(acceptor));
        }

        if contract.acceptances.contains(&acceptor) {
            return Err(ContractError::AlreadyAccepted(acceptor));
        }

        contract.acceptances.push(acceptor);

        // If all counterparties have accepted, activate
        if contract.acceptances.len() == contract.counterparties.len() {
            contract.status = ContractStatus::Active;
            contract.activated_at = Some(Utc::now());
        }

        Ok(contract.status)
    }

    /// Reject a contract (counterparty declines).
    pub fn reject(&self, contract_id: ContractId) -> Result<(), ContractError> {
        let mut contract = self
            .contracts
            .get_mut(&contract_id)
            .ok_or(ContractError::NotFound(contract_id))?;

        if contract.status != ContractStatus::Proposed {
            return Err(ContractError::InvalidStatus {
                expected: ContractStatus::Proposed,
                actual: contract.status,
            });
        }

        contract.status = ContractStatus::Cancelled;
        Ok(())
    }

    /// Fulfill a specific obligation.
    pub fn fulfill_obligation(
        &self,
        contract_id: ContractId,
        obligation_id: Uuid,
        evidence: String,
    ) -> Result<(), ContractError> {
        let mut contract = self
            .contracts
            .get_mut(&contract_id)
            .ok_or(ContractError::NotFound(contract_id))?;

        if contract.status != ContractStatus::Active {
            return Err(ContractError::InvalidStatus {
                expected: ContractStatus::Active,
                actual: contract.status,
            });
        }

        // Search proposer obligations first, then counterparty
        let found_in_proposer = contract
            .proposer_obligations
            .iter_mut()
            .find(|o| o.id == obligation_id);

        if let Some(obligation) = found_in_proposer {
            obligation.fulfilled = true;
            obligation.evidence = Some(evidence);
            return Ok(());
        }

        let found_in_counterparty = contract
            .counterparty_obligations
            .iter_mut()
            .find(|o| o.id == obligation_id);

        match found_in_counterparty {
            Some(obligation) => {
                obligation.fulfilled = true;
                obligation.evidence = Some(evidence);
                Ok(())
            }
            None => Err(ContractError::ObligationNotFound(obligation_id)),
        }
    }

    /// Check if all obligations are fulfilled and settle the contract.
    pub fn try_settle(&self, contract_id: ContractId) -> Result<ContractStatus, ContractError> {
        let mut contract = self
            .contracts
            .get_mut(&contract_id)
            .ok_or(ContractError::NotFound(contract_id))?;

        if contract.status != ContractStatus::Active {
            return Err(ContractError::InvalidStatus {
                expected: ContractStatus::Active,
                actual: contract.status,
            });
        }

        let all_fulfilled = contract
            .proposer_obligations
            .iter()
            .chain(contract.counterparty_obligations.iter())
            .all(|o| o.fulfilled);

        if all_fulfilled {
            contract.status = ContractStatus::Fulfilled;
            contract.settled_at = Some(Utc::now());
            Ok(ContractStatus::Fulfilled)
        } else {
            Ok(ContractStatus::Active)
        }
    }

    /// Report a breach of contract.
    pub fn report_breach(
        &self,
        contract_id: ContractId,
        reason: String,
    ) -> Result<(), ContractError> {
        let mut contract = self
            .contracts
            .get_mut(&contract_id)
            .ok_or(ContractError::NotFound(contract_id))?;

        if contract.status != ContractStatus::Active {
            return Err(ContractError::InvalidStatus {
                expected: ContractStatus::Active,
                actual: contract.status,
            });
        }

        let dispute = Dispute {
            id: Uuid::new_v4(),
            raised_by: self.agent_id,
            reason,
            evidence: None,
            raised_at: Utc::now(),
            resolved: false,
            resolution: None,
        };

        contract.disputes.push(dispute);
        contract.status = ContractStatus::Disputed;
        Ok(())
    }

    /// Check for expired contracts and mark them.
    pub fn check_expirations(&self) -> Vec<ContractId> {
        let now = Utc::now();
        let mut expired = Vec::new();

        for mut entry in self.contracts.iter_mut() {
            let contract = entry.value_mut();
            if contract.status == ContractStatus::Active {
                if let Some(deadline) = contract.sla.deadline {
                    if now > deadline {
                        contract.status = ContractStatus::Expired;
                        contract.settled_at = Some(now);
                        expired.push(contract.id);
                    }
                }
            }
        }

        expired
    }

    /// Record resource consumption against a contract's budget.
    pub fn record_usage(
        &self,
        contract_id: ContractId,
        usage: ResourceUsage,
    ) -> Result<bool, ContractError> {
        let mut contract = self
            .contracts
            .get_mut(&contract_id)
            .ok_or(ContractError::NotFound(contract_id))?;

        let budget = &mut contract.resource_budget;
        budget.consumed.compute_used += usage.compute_used;
        budget.consumed.memory_bytes_used += usage.memory_bytes_used;
        budget.consumed.bandwidth_bytes_used += usage.bandwidth_bytes_used;
        budget.consumed.api_calls_used += usage.api_calls_used;
        budget.consumed.credits_used += usage.credits_used;

        Ok(budget.is_exceeded())
    }

    /// Get a contract by ID.
    pub fn get(&self, contract_id: &ContractId) -> Option<Contract> {
        self.contracts.get(contract_id).map(|c| c.clone())
    }

    /// List all contracts for this agent (as proposer or counterparty).
    pub fn list_contracts(&self) -> Vec<Contract> {
        self.contracts.iter().map(|e| e.value().clone()).collect()
    }

    /// List contracts by status.
    pub fn list_by_status(&self, status: ContractStatus) -> Vec<Contract> {
        self.contracts
            .iter()
            .filter(|e| e.value().status == status)
            .map(|e| e.value().clone())
            .collect()
    }

    /// Get summary statistics.
    pub fn stats(&self) -> ContractStats {
        let mut by_status = HashMap::new();
        let mut total_budget = 0.0;
        let mut total_consumed = 0.0;

        for entry in self.contracts.iter() {
            *by_status.entry(entry.value().status).or_insert(0usize) += 1;
            total_budget += entry.value().resource_budget.credit_budget;
            total_consumed += entry.value().resource_budget.consumed.credits_used;
        }

        ContractStats {
            total_contracts: self.contracts.len(),
            by_status,
            total_credit_budget: total_budget,
            total_credits_consumed: total_consumed,
        }
    }

    /// Verify the integrity of a contract's terms.
    pub fn verify_integrity(&self, contract_id: &ContractId) -> Result<bool, ContractError> {
        let contract = self
            .contracts
            .get(contract_id)
            .ok_or(ContractError::NotFound(*contract_id))?;

        let computed = compute_terms_hash(
            &contract.name,
            &contract.proposer,
            &contract.counterparties,
            &contract.proposer_obligations,
            &contract.counterparty_obligations,
        );

        Ok(computed == contract.terms_hash)
    }
}

/// Compute a SHA-256 hash of the contract terms for integrity checking.
fn compute_terms_hash(
    name: &str,
    proposer: &AgentId,
    counterparties: &[AgentId],
    proposer_obligations: &[Obligation],
    counterparty_obligations: &[Obligation],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(name.as_bytes());
    hasher.update(proposer.0.as_bytes());
    for cp in counterparties {
        hasher.update(cp.0.as_bytes());
    }
    for ob in proposer_obligations {
        hasher.update(ob.description.as_bytes());
    }
    for ob in counterparty_obligations {
        hasher.update(ob.description.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

// =============================================================================
// CONTRACT STATS & ERRORS
// =============================================================================

/// Contract statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractStats {
    pub total_contracts: usize,
    pub by_status: HashMap<ContractStatus, usize>,
    pub total_credit_budget: f64,
    pub total_credits_consumed: f64,
}

/// Errors during contract operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContractError {
    NotFound(ContractId),
    InvalidStatus {
        expected: ContractStatus,
        actual: ContractStatus,
    },
    NotAParty(AgentId),
    AlreadyAccepted(AgentId),
    ObligationNotFound(Uuid),
    BudgetExceeded,
}

impl std::fmt::Display for ContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "Contract not found: {}", id),
            Self::InvalidStatus { expected, actual } => {
                write!(f, "Expected status {:?}, got {:?}", expected, actual)
            }
            Self::NotAParty(id) => write!(f, "Agent {} is not a party to this contract", id.0),
            Self::AlreadyAccepted(id) => write!(f, "Agent {} already accepted", id.0),
            Self::ObligationNotFound(id) => write!(f, "Obligation not found: {}", id),
            Self::BudgetExceeded => write!(f, "Resource budget exceeded"),
        }
    }
}

impl std::error::Error for ContractError {}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_obligation(desc: &str) -> Obligation {
        Obligation {
            id: Uuid::new_v4(),
            description: desc.to_string(),
            obligation_type: ObligationType::TaskExecution {
                task_description: desc.to_string(),
                required_capabilities: vec![],
            },
            deadline: None,
            fulfilled: false,
            evidence: None,
        }
    }

    #[test]
    fn test_propose_contract() {
        let agent_a = AgentId::new();
        let agent_b = AgentId::new();
        let mgr = ContractManager::new(agent_a);

        let contract = mgr.propose(
            "Data Processing",
            vec![agent_b],
            vec![make_obligation("pay 10 credits")],
            vec![make_obligation("process dataset")],
            ContractSla::default(),
            ResourceBudget::default(),
        );

        assert_eq!(contract.status, ContractStatus::Proposed);
        assert_eq!(contract.proposer, agent_a);
        assert!(!contract.terms_hash.is_empty());
    }

    #[test]
    fn test_accept_and_activate() {
        let agent_a = AgentId::new();
        let agent_b = AgentId::new();
        let mgr = ContractManager::new(agent_a);

        let contract = mgr.propose(
            "Test",
            vec![agent_b],
            vec![make_obligation("A provides")],
            vec![make_obligation("B delivers")],
            ContractSla::default(),
            ResourceBudget::default(),
        );

        let status = mgr.accept(contract.id, agent_b).unwrap();
        assert_eq!(status, ContractStatus::Active);
    }

    #[test]
    fn test_reject_contract() {
        let agent_a = AgentId::new();
        let agent_b = AgentId::new();
        let mgr = ContractManager::new(agent_a);

        let contract = mgr.propose(
            "Test",
            vec![agent_b],
            vec![],
            vec![],
            ContractSla::default(),
            ResourceBudget::default(),
        );

        mgr.reject(contract.id).unwrap();
        assert_eq!(mgr.get(&contract.id).unwrap().status, ContractStatus::Cancelled);
    }

    #[test]
    fn test_multi_party_acceptance() {
        let agent_a = AgentId::new();
        let agent_b = AgentId::new();
        let agent_c = AgentId::new();
        let mgr = ContractManager::new(agent_a);

        let contract = mgr.propose(
            "Multi-party",
            vec![agent_b, agent_c],
            vec![],
            vec![],
            ContractSla::default(),
            ResourceBudget::default(),
        );

        // First acceptance: still Proposed
        let status = mgr.accept(contract.id, agent_b).unwrap();
        assert_eq!(status, ContractStatus::Proposed);

        // Second acceptance: now Active
        let status = mgr.accept(contract.id, agent_c).unwrap();
        assert_eq!(status, ContractStatus::Active);
    }

    #[test]
    fn test_fulfill_and_settle() {
        let agent_a = AgentId::new();
        let agent_b = AgentId::new();
        let mgr = ContractManager::new(agent_a);

        let ob1 = make_obligation("A pays");
        let ob2 = make_obligation("B delivers");
        let ob1_id = ob1.id;
        let ob2_id = ob2.id;

        let contract = mgr.propose(
            "Test",
            vec![agent_b],
            vec![ob1],
            vec![ob2],
            ContractSla::default(),
            ResourceBudget::default(),
        );

        mgr.accept(contract.id, agent_b).unwrap();

        // Not yet settled
        assert_eq!(mgr.try_settle(contract.id).unwrap(), ContractStatus::Active);

        // Fulfill both obligations
        mgr.fulfill_obligation(contract.id, ob1_id, "payment_hash".to_string())
            .unwrap();
        mgr.fulfill_obligation(contract.id, ob2_id, "delivery_hash".to_string())
            .unwrap();

        // Now it should settle
        assert_eq!(mgr.try_settle(contract.id).unwrap(), ContractStatus::Fulfilled);
    }

    #[test]
    fn test_breach_and_dispute() {
        let agent_a = AgentId::new();
        let agent_b = AgentId::new();
        let mgr = ContractManager::new(agent_a);

        let contract = mgr.propose(
            "Test",
            vec![agent_b],
            vec![],
            vec![],
            ContractSla::default(),
            ResourceBudget::default(),
        );
        mgr.accept(contract.id, agent_b).unwrap();

        mgr.report_breach(contract.id, "Delivered garbage".to_string())
            .unwrap();

        let c = mgr.get(&contract.id).unwrap();
        assert_eq!(c.status, ContractStatus::Disputed);
        assert_eq!(c.disputes.len(), 1);
    }

    #[test]
    fn test_resource_budget_tracking() {
        let agent_a = AgentId::new();
        let agent_b = AgentId::new();
        let mgr = ContractManager::new(agent_a);

        let contract = mgr.propose(
            "Budget test",
            vec![agent_b],
            vec![],
            vec![],
            ContractSla::default(),
            ResourceBudget {
                credit_budget: 100.0,
                max_api_calls: Some(10),
                ..Default::default()
            },
        );
        mgr.accept(contract.id, agent_b).unwrap();

        let exceeded = mgr
            .record_usage(
                contract.id,
                ResourceUsage {
                    credits_used: 50.0,
                    api_calls_used: 5,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(!exceeded);

        let exceeded = mgr
            .record_usage(
                contract.id,
                ResourceUsage {
                    credits_used: 60.0,
                    api_calls_used: 6,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(exceeded); // 110 credits > 100 limit, 11 calls > 10 limit
    }

    #[test]
    fn test_verify_integrity() {
        let agent_a = AgentId::new();
        let agent_b = AgentId::new();
        let mgr = ContractManager::new(agent_a);

        let contract = mgr.propose(
            "Integrity",
            vec![agent_b],
            vec![make_obligation("test")],
            vec![],
            ContractSla::default(),
            ResourceBudget::default(),
        );

        assert!(mgr.verify_integrity(&contract.id).unwrap());
    }

    #[test]
    fn test_contract_stats() {
        let agent_a = AgentId::new();
        let agent_b = AgentId::new();
        let mgr = ContractManager::new(agent_a);

        mgr.propose(
            "A",
            vec![agent_b],
            vec![],
            vec![],
            ContractSla::default(),
            ResourceBudget {
                credit_budget: 50.0,
                ..Default::default()
            },
        );
        mgr.propose(
            "B",
            vec![agent_b],
            vec![],
            vec![],
            ContractSla::default(),
            ResourceBudget {
                credit_budget: 100.0,
                ..Default::default()
            },
        );

        let stats = mgr.stats();
        assert_eq!(stats.total_contracts, 2);
        assert_eq!(stats.total_credit_budget, 150.0);
    }

    #[test]
    fn test_list_by_status() {
        let agent_a = AgentId::new();
        let agent_b = AgentId::new();
        let mgr = ContractManager::new(agent_a);

        let c1 = mgr.propose("A", vec![agent_b], vec![], vec![], ContractSla::default(), ResourceBudget::default());
        let _c2 = mgr.propose("B", vec![agent_b], vec![], vec![], ContractSla::default(), ResourceBudget::default());

        mgr.accept(c1.id, agent_b).unwrap();

        assert_eq!(mgr.list_by_status(ContractStatus::Active).len(), 1);
        assert_eq!(mgr.list_by_status(ContractStatus::Proposed).len(), 1);
    }

    #[test]
    fn test_cannot_accept_twice() {
        let agent_a = AgentId::new();
        let agent_b = AgentId::new();
        let mgr = ContractManager::new(agent_a);

        let contract = mgr.propose("Test", vec![agent_b], vec![], vec![], ContractSla::default(), ResourceBudget::default());
        mgr.accept(contract.id, agent_b).unwrap();

        let result = mgr.accept(contract.id, agent_b);
        assert!(result.is_err());
    }

    #[test]
    fn test_non_party_cannot_accept() {
        let agent_a = AgentId::new();
        let agent_b = AgentId::new();
        let agent_c = AgentId::new();
        let mgr = ContractManager::new(agent_a);

        let contract = mgr.propose("Test", vec![agent_b], vec![], vec![], ContractSla::default(), ResourceBudget::default());
        let result = mgr.accept(contract.id, agent_c);
        assert!(matches!(result, Err(ContractError::NotAParty(_))));
    }
}
