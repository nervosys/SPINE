// =============================================================================
// FEDERATION: Cross-Swarm Trust Boundary Enforcement
// =============================================================================
//
// Enables separate swarms to cooperate while maintaining security boundaries.
// Federated identity, cross-swarm contracts, and capability-based access control.

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::AgentId;

/// A unique identifier for a swarm in the federation.
pub type SwarmId = Uuid;

/// A federated swarm registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederatedSwarm {
    pub id: SwarmId,
    pub name: String,
    pub public_key_hash: [u8; 32],
    pub endpoint: String,
    pub capabilities_offered: HashSet<String>,
    pub capabilities_required: HashSet<String>,
    pub trust_level: FederationTrust,
    pub joined_at: DateTime<Utc>,
    pub last_heartbeat: DateTime<Utc>,
    pub metadata: HashMap<String, String>,
}

/// Trust level between federated swarms.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum FederationTrust {
    /// No trust — no cross-swarm operations allowed.
    Untrusted,
    /// Limited trust — read-only queries, no task delegation.
    Observer,
    /// Standard trust — task delegation with capability checks.
    Participant,
    /// High trust — full bidirectional cooperation.
    Ally,
    /// Maximum trust — shared resources and joint decisions.
    Integrated,
}

/// A cross-swarm access policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationPolicy {
    pub id: Uuid,
    pub source_swarm: SwarmId,
    pub target_swarm: SwarmId,
    pub permissions: HashSet<FederationPermission>,
    pub conditions: Vec<PolicyCondition>,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

/// Permitted cross-swarm operations.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FederationPermission {
    /// Query the remote swarm's capabilities.
    DiscoverCapabilities,
    /// Send task requests.
    DelegateTasks,
    /// Receive task results.
    ReceiveResults,
    /// Share pheromone signals (stigmergy).
    ShareSignals,
    /// Participate in cross-swarm consensus.
    JoinConsensus,
    /// Migrate agents between swarms.
    AgentMigration,
    /// Access shared knowledge base.
    SharedKnowledge,
    /// Custom permission.
    Custom(String),
}

/// Conditions that must be met for a policy to apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PolicyCondition {
    /// Minimum trust level required.
    MinTrust(FederationTrust),
    /// Only during specific time windows.
    TimeWindow { start: DateTime<Utc>, end: DateTime<Utc> },
    /// Maximum concurrent delegated tasks.
    MaxConcurrentTasks(usize),
    /// Required capability on the requesting side.
    RequiresCapability(String),
    /// Rate limit (operations per minute).
    RateLimit(u32),
}

/// A cross-swarm task delegation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationRequest {
    pub id: Uuid,
    pub from_swarm: SwarmId,
    pub to_swarm: SwarmId,
    pub requesting_agent: AgentId,
    pub operation: FederationPermission,
    pub payload: String,
    pub created_at: DateTime<Utc>,
    pub status: RequestStatus,
    /// SHA-256 of the request for integrity verification.
    pub integrity_hash: [u8; 32],
}

/// Status of a federation request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestStatus {
    Pending,
    Approved,
    Denied(String),
    InProgress,
    Completed,
    Failed(String),
}

/// Error type for federation operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FederationError {
    SwarmNotFound,
    SwarmAlreadyRegistered,
    InsufficientTrust,
    PermissionDenied,
    PolicyViolation(String),
    RequestNotFound,
    SwarmUnreachable,
}

impl std::fmt::Display for FederationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SwarmNotFound => write!(f, "swarm not found in federation"),
            Self::SwarmAlreadyRegistered => write!(f, "swarm already registered"),
            Self::InsufficientTrust => write!(f, "insufficient trust level"),
            Self::PermissionDenied => write!(f, "permission denied by federation policy"),
            Self::PolicyViolation(msg) => write!(f, "policy violation: {msg}"),
            Self::RequestNotFound => write!(f, "federation request not found"),
            Self::SwarmUnreachable => write!(f, "target swarm unreachable"),
        }
    }
}

impl std::error::Error for FederationError {}

/// Manages cross-swarm federation with trust boundaries.
pub struct FederationManager {
    /// This swarm's identity.
    local_swarm_id: SwarmId,
    /// Registered federated swarms.
    swarms: DashMap<SwarmId, FederatedSwarm>,
    /// Access policies between swarms.
    policies: DashMap<Uuid, FederationPolicy>,
    /// Pending and completed requests.
    requests: DashMap<Uuid, FederationRequest>,
    /// Per-source rate tracking (swarm_id → count in current window).
    rate_counters: DashMap<SwarmId, (u32, DateTime<Utc>)>,
}

impl FederationManager {
    pub fn new(local_swarm_id: SwarmId) -> Self {
        Self {
            local_swarm_id,
            swarms: DashMap::new(),
            policies: DashMap::new(),
            requests: DashMap::new(),
            rate_counters: DashMap::new(),
        }
    }

    /// Register a remote swarm in the federation.
    pub fn register_swarm(&self, swarm: FederatedSwarm) -> Result<(), FederationError> {
        if self.swarms.contains_key(&swarm.id) {
            return Err(FederationError::SwarmAlreadyRegistered);
        }
        self.swarms.insert(swarm.id, swarm);
        Ok(())
    }

    /// Update a swarm's trust level.
    pub fn set_trust(&self, swarm_id: SwarmId, trust: FederationTrust) -> Result<(), FederationError> {
        let mut swarm = self
            .swarms
            .get_mut(&swarm_id)
            .ok_or(FederationError::SwarmNotFound)?;
        swarm.trust_level = trust;
        Ok(())
    }

    /// Record a heartbeat from a remote swarm.
    pub fn heartbeat(&self, swarm_id: SwarmId) -> Result<(), FederationError> {
        let mut swarm = self
            .swarms
            .get_mut(&swarm_id)
            .ok_or(FederationError::SwarmNotFound)?;
        swarm.last_heartbeat = Utc::now();
        Ok(())
    }

    /// Create a cross-swarm access policy.
    pub fn create_policy(&self, policy: FederationPolicy) -> Uuid {
        let id = policy.id;
        self.policies.insert(id, policy);
        id
    }

    /// Check whether a cross-swarm operation is permitted.
    pub fn check_permission(
        &self,
        from_swarm: SwarmId,
        to_swarm: SwarmId,
        permission: &FederationPermission,
    ) -> Result<(), FederationError> {
        let source = self
            .swarms
            .get(&from_swarm)
            .ok_or(FederationError::SwarmNotFound)?;

        // Check base trust level
        match permission {
            FederationPermission::DiscoverCapabilities => {
                if source.trust_level < FederationTrust::Observer {
                    return Err(FederationError::InsufficientTrust);
                }
            }
            FederationPermission::DelegateTasks
            | FederationPermission::ReceiveResults
            | FederationPermission::ShareSignals => {
                if source.trust_level < FederationTrust::Participant {
                    return Err(FederationError::InsufficientTrust);
                }
            }
            FederationPermission::JoinConsensus
            | FederationPermission::SharedKnowledge => {
                if source.trust_level < FederationTrust::Ally {
                    return Err(FederationError::InsufficientTrust);
                }
            }
            FederationPermission::AgentMigration => {
                if source.trust_level < FederationTrust::Integrated {
                    return Err(FederationError::InsufficientTrust);
                }
            }
            FederationPermission::Custom(_) => {
                if source.trust_level < FederationTrust::Participant {
                    return Err(FederationError::InsufficientTrust);
                }
            }
        }
        drop(source);

        // Check specific policies
        let matching_policies: Vec<_> = self
            .policies
            .iter()
            .filter(|p| {
                let pol = p.value();
                pol.source_swarm == from_swarm
                    && pol.target_swarm == to_swarm
                    && pol.permissions.contains(permission)
            })
            .collect();

        if matching_policies.is_empty() {
            return Err(FederationError::PermissionDenied);
        }

        // Verify conditions
        let now = Utc::now();
        for policy_ref in &matching_policies {
            let pol = policy_ref.value();

            // Check expiry
            if let Some(exp) = pol.expires_at {
                if now > exp {
                    continue;
                }
            }

            // Check conditions
            let conditions_met = pol.conditions.iter().all(|c| match c {
                PolicyCondition::MinTrust(min) => {
                    self.swarms
                        .get(&from_swarm)
                        .map(|s| s.trust_level >= *min)
                        .unwrap_or(false)
                }
                PolicyCondition::TimeWindow { start, end } => now >= *start && now <= *end,
                PolicyCondition::MaxConcurrentTasks(max) => {
                    let active = self
                        .requests
                        .iter()
                        .filter(|r| {
                            r.from_swarm == from_swarm
                                && matches!(r.status, RequestStatus::InProgress)
                        })
                        .count();
                    active < *max
                }
                PolicyCondition::RequiresCapability(cap) => {
                    self.swarms
                        .get(&from_swarm)
                        .map(|s| s.capabilities_offered.contains(cap))
                        .unwrap_or(false)
                }
                PolicyCondition::RateLimit(max_per_min) => {
                    self.check_rate(from_swarm, *max_per_min)
                }
            });

            if conditions_met {
                return Ok(());
            }
        }

        Err(FederationError::PolicyViolation(
            "no valid policy with satisfied conditions".into(),
        ))
    }

    /// Submit a cross-swarm request.
    pub fn submit_request(
        &self,
        from_swarm: SwarmId,
        to_swarm: SwarmId,
        requesting_agent: AgentId,
        operation: FederationPermission,
        payload: String,
    ) -> Result<Uuid, FederationError> {
        // Enforce policy
        self.check_permission(from_swarm, to_swarm, &operation)?;

        // Update rate counter
        self.increment_rate(from_swarm);

        let id = Uuid::new_v4();
        let integrity_hash = Self::hash_request(&from_swarm, &to_swarm, &payload);

        let request = FederationRequest {
            id,
            from_swarm,
            to_swarm,
            requesting_agent,
            operation,
            payload,
            created_at: Utc::now(),
            status: RequestStatus::Pending,
            integrity_hash,
        };

        self.requests.insert(id, request);
        Ok(id)
    }

    /// Approve a pending request.
    pub fn approve_request(&self, request_id: Uuid) -> Result<(), FederationError> {
        let mut req = self
            .requests
            .get_mut(&request_id)
            .ok_or(FederationError::RequestNotFound)?;

        if req.status != RequestStatus::Pending {
            return Err(FederationError::RequestNotFound);
        }

        req.status = RequestStatus::Approved;
        Ok(())
    }

    /// Deny a pending request.
    pub fn deny_request(&self, request_id: Uuid, reason: String) -> Result<(), FederationError> {
        let mut req = self
            .requests
            .get_mut(&request_id)
            .ok_or(FederationError::RequestNotFound)?;

        req.status = RequestStatus::Denied(reason);
        Ok(())
    }

    /// Complete a request.
    pub fn complete_request(&self, request_id: Uuid) -> Result<(), FederationError> {
        let mut req = self
            .requests
            .get_mut(&request_id)
            .ok_or(FederationError::RequestNotFound)?;

        req.status = RequestStatus::Completed;
        Ok(())
    }

    /// Discover capabilities available across the federation.
    pub fn discover_capabilities(&self) -> HashMap<SwarmId, HashSet<String>> {
        self.swarms
            .iter()
            .filter(|e| e.trust_level >= FederationTrust::Observer)
            .map(|e| (e.id, e.capabilities_offered.clone()))
            .collect()
    }

    /// Find swarms that offer a specific capability.
    pub fn find_capability(&self, capability: &str) -> Vec<SwarmId> {
        self.swarms
            .iter()
            .filter(|e| {
                e.trust_level >= FederationTrust::Participant
                    && e.capabilities_offered.contains(capability)
            })
            .map(|e| e.id)
            .collect()
    }

    /// Get the local swarm ID.
    pub fn local_id(&self) -> SwarmId {
        self.local_swarm_id
    }

    /// List all registered swarms.
    pub fn list_swarms(&self) -> Vec<FederatedSwarm> {
        self.swarms.iter().map(|e| e.value().clone()).collect()
    }

    /// Get a swarm by ID.
    pub fn get_swarm(&self, id: SwarmId) -> Option<FederatedSwarm> {
        self.swarms.get(&id).map(|s| s.clone())
    }

    /// Remove a swarm from the federation.
    pub fn remove_swarm(&self, id: SwarmId) -> Result<(), FederationError> {
        self.swarms.remove(&id).ok_or(FederationError::SwarmNotFound)?;
        // Clean up policies
        let to_remove: Vec<Uuid> = self
            .policies
            .iter()
            .filter(|p| p.source_swarm == id || p.target_swarm == id)
            .map(|p| p.id)
            .collect();
        for pid in to_remove {
            self.policies.remove(&pid);
        }
        Ok(())
    }

    fn check_rate(&self, swarm_id: SwarmId, max_per_min: u32) -> bool {
        let now = Utc::now();
        if let Some(entry) = self.rate_counters.get(&swarm_id) {
            let (count, window_start) = *entry;
            if (now - window_start).num_seconds() < 60 {
                return count < max_per_min;
            }
        }
        true
    }

    fn increment_rate(&self, swarm_id: SwarmId) {
        let now = Utc::now();
        self.rate_counters
            .entry(swarm_id)
            .and_modify(|(count, window_start)| {
                if (now - *window_start).num_seconds() >= 60 {
                    *count = 1;
                    *window_start = now;
                } else {
                    *count += 1;
                }
            })
            .or_insert((1, now));
    }

    fn hash_request(from: &SwarmId, to: &SwarmId, payload: &str) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(from.as_bytes());
        hasher.update(to.as_bytes());
        hasher.update(payload.as_bytes());
        hasher.finalize().into()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_swarm(name: &str, trust: FederationTrust, caps: &[&str]) -> FederatedSwarm {
        FederatedSwarm {
            id: Uuid::new_v4(),
            name: name.to_string(),
            public_key_hash: [0u8; 32],
            endpoint: format!("tcp://{}:9000", name),
            capabilities_offered: caps.iter().map(|s| s.to_string()).collect(),
            capabilities_required: HashSet::new(),
            trust_level: trust,
            joined_at: Utc::now(),
            last_heartbeat: Utc::now(),
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn test_register_and_list() {
        let fm = FederationManager::new(Uuid::new_v4());
        let s = make_swarm("alpha", FederationTrust::Participant, &["search"]);
        let sid = s.id;
        fm.register_swarm(s).unwrap();

        assert_eq!(fm.list_swarms().len(), 1);
        assert!(fm.get_swarm(sid).is_some());
    }

    #[test]
    fn test_duplicate_registration() {
        let fm = FederationManager::new(Uuid::new_v4());
        let s = make_swarm("alpha", FederationTrust::Participant, &[]);
        let s2 = s.clone();
        fm.register_swarm(s).unwrap();
        assert_eq!(
            fm.register_swarm(s2),
            Err(FederationError::SwarmAlreadyRegistered)
        );
    }

    #[test]
    fn test_trust_level_enforcement() {
        let fm = FederationManager::new(Uuid::new_v4());
        let source = make_swarm("src", FederationTrust::Observer, &[]);
        let target = make_swarm("tgt", FederationTrust::Participant, &[]);
        let src_id = source.id;
        let tgt_id = target.id;
        fm.register_swarm(source).unwrap();
        fm.register_swarm(target).unwrap();

        // Observer can discover but not delegate
        assert!(fm
            .check_permission(src_id, tgt_id, &FederationPermission::DiscoverCapabilities)
            .is_err()); // No policy yet, but trust is enough — policy check fails

        // Even with policy, delegation should fail for Observer
        fm.create_policy(FederationPolicy {
            id: Uuid::new_v4(),
            source_swarm: src_id,
            target_swarm: tgt_id,
            permissions: HashSet::from([FederationPermission::DelegateTasks]),
            conditions: Vec::new(),
            expires_at: None,
            created_at: Utc::now(),
        });

        assert_eq!(
            fm.check_permission(src_id, tgt_id, &FederationPermission::DelegateTasks),
            Err(FederationError::InsufficientTrust)
        );
    }

    #[test]
    fn test_policy_permits_operation() {
        let fm = FederationManager::new(Uuid::new_v4());
        let source = make_swarm("src", FederationTrust::Participant, &["search"]);
        let target = make_swarm("tgt", FederationTrust::Participant, &["analyze"]);
        let src_id = source.id;
        let tgt_id = target.id;
        fm.register_swarm(source).unwrap();
        fm.register_swarm(target).unwrap();

        fm.create_policy(FederationPolicy {
            id: Uuid::new_v4(),
            source_swarm: src_id,
            target_swarm: tgt_id,
            permissions: HashSet::from([FederationPermission::DelegateTasks]),
            conditions: Vec::new(),
            expires_at: None,
            created_at: Utc::now(),
        });

        assert!(fm
            .check_permission(src_id, tgt_id, &FederationPermission::DelegateTasks)
            .is_ok());
    }

    #[test]
    fn test_submit_and_approve_request() {
        let fm = FederationManager::new(Uuid::new_v4());
        let source = make_swarm("src", FederationTrust::Participant, &[]);
        let target = make_swarm("tgt", FederationTrust::Participant, &[]);
        let src_id = source.id;
        let tgt_id = target.id;
        fm.register_swarm(source).unwrap();
        fm.register_swarm(target).unwrap();

        fm.create_policy(FederationPolicy {
            id: Uuid::new_v4(),
            source_swarm: src_id,
            target_swarm: tgt_id,
            permissions: HashSet::from([FederationPermission::DelegateTasks]),
            conditions: Vec::new(),
            expires_at: None,
            created_at: Utc::now(),
        });

        let rid = fm
            .submit_request(
                src_id,
                tgt_id,
                AgentId::new(),
                FederationPermission::DelegateTasks,
                "analyze data".into(),
            )
            .unwrap();

        fm.approve_request(rid).unwrap();
        let req = fm.requests.get(&rid).unwrap();
        assert_eq!(req.status, RequestStatus::Approved);
    }

    #[test]
    fn test_discover_capabilities() {
        let fm = FederationManager::new(Uuid::new_v4());
        fm.register_swarm(make_swarm("a", FederationTrust::Participant, &["search", "crawl"]))
            .unwrap();
        fm.register_swarm(make_swarm("b", FederationTrust::Ally, &["analyze"]))
            .unwrap();
        fm.register_swarm(make_swarm("c", FederationTrust::Untrusted, &["secret"]))
            .unwrap();

        let caps = fm.discover_capabilities();
        assert_eq!(caps.len(), 2); // untrusted excluded
    }

    #[test]
    fn test_find_capability() {
        let fm = FederationManager::new(Uuid::new_v4());
        fm.register_swarm(make_swarm("a", FederationTrust::Participant, &["search"]))
            .unwrap();
        fm.register_swarm(make_swarm("b", FederationTrust::Participant, &["search", "analyze"]))
            .unwrap();
        fm.register_swarm(make_swarm("c", FederationTrust::Observer, &["search"]))
            .unwrap();

        let providers = fm.find_capability("search");
        assert_eq!(providers.len(), 2); // observer excluded
    }

    #[test]
    fn test_remove_swarm_cleans_policies() {
        let fm = FederationManager::new(Uuid::new_v4());
        let s1 = make_swarm("a", FederationTrust::Participant, &[]);
        let s2 = make_swarm("b", FederationTrust::Participant, &[]);
        let s1_id = s1.id;
        let s2_id = s2.id;
        fm.register_swarm(s1).unwrap();
        fm.register_swarm(s2).unwrap();

        fm.create_policy(FederationPolicy {
            id: Uuid::new_v4(),
            source_swarm: s1_id,
            target_swarm: s2_id,
            permissions: HashSet::from([FederationPermission::DelegateTasks]),
            conditions: Vec::new(),
            expires_at: None,
            created_at: Utc::now(),
        });

        fm.remove_swarm(s1_id).unwrap();
        assert!(fm.get_swarm(s1_id).is_none());
        assert!(fm.policies.is_empty());
    }

    #[test]
    fn test_heartbeat() {
        let fm = FederationManager::new(Uuid::new_v4());
        let s = make_swarm("a", FederationTrust::Participant, &[]);
        let sid = s.id;
        fm.register_swarm(s).unwrap();

        let before = fm.get_swarm(sid).unwrap().last_heartbeat;
        std::thread::sleep(std::time::Duration::from_millis(10));
        fm.heartbeat(sid).unwrap();
        let after = fm.get_swarm(sid).unwrap().last_heartbeat;
        assert!(after > before);
    }

    #[test]
    fn test_set_trust() {
        let fm = FederationManager::new(Uuid::new_v4());
        let s = make_swarm("a", FederationTrust::Observer, &[]);
        let sid = s.id;
        fm.register_swarm(s).unwrap();

        fm.set_trust(sid, FederationTrust::Ally).unwrap();
        assert_eq!(fm.get_swarm(sid).unwrap().trust_level, FederationTrust::Ally);
    }

    #[test]
    fn test_request_integrity_hash() {
        let fm = FederationManager::new(Uuid::new_v4());
        let source = make_swarm("src", FederationTrust::Participant, &[]);
        let target = make_swarm("tgt", FederationTrust::Participant, &[]);
        let src_id = source.id;
        let tgt_id = target.id;
        fm.register_swarm(source).unwrap();
        fm.register_swarm(target).unwrap();

        fm.create_policy(FederationPolicy {
            id: Uuid::new_v4(),
            source_swarm: src_id,
            target_swarm: tgt_id,
            permissions: HashSet::from([FederationPermission::DelegateTasks]),
            conditions: Vec::new(),
            expires_at: None,
            created_at: Utc::now(),
        });

        let rid = fm
            .submit_request(
                src_id,
                tgt_id,
                AgentId::new(),
                FederationPermission::DelegateTasks,
                "test payload".into(),
            )
            .unwrap();

        let req = fm.requests.get(&rid).unwrap();
        assert_ne!(req.integrity_hash, [0u8; 32]);
    }
}
