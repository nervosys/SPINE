//! Agent Capability Marketplace
//!
//! A decentralized registry where agents advertise capabilities (skills, tools,
//! models) and discover peers that can fulfill specific tasks. Enables dynamic
//! composition of multi-agent workflows.
//!
//! # Architecture
//!
//! ```text
//! ┌───────────────────────────────────────────────────────────────┐
//! │                  Capability Marketplace                       │
//! │  ┌─────────────┐  ┌──────────────┐  ┌──────────────────────┐│
//! │  │  Registry    │  │  Discovery   │  │  Reputation          ││
//! │  │  (DashMap)   │  │  (search +   │  │  (success rate,      ││
//! │  │             │  │   match)      │  │   latency, reviews)  ││
//! │  └─────────────┘  └──────────────┘  └──────────────────────┘│
//! │  ┌─────────────┐  ┌──────────────┐  ┌──────────────────────┐│
//! │  │  Bidding    │  │  Contracts   │  │  Audit Log           ││
//! │  │  (auction   │  │  (SLA +      │  │  (immutable trace)   ││
//! │  │   engine)   │  │   escrow)    │  │                      ││
//! │  └─────────────┘  └──────────────┘  └──────────────────────┘│
//! └───────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! let marketplace = Marketplace::new();
//!
//! // Agent advertises capabilities
//! marketplace.register(AgentListing {
//!     agent_id: "agent-007".into(),
//!     capabilities: vec![
//!         Capability::new("web-scraping", "Extracts structured data from web pages")
//!             .with_tag("html").with_tag("parsing")
//!             .with_price(0.01),
//!     ],
//!     ..Default::default()
//! });
//!
//! // Another agent discovers and bids
//! let matches = marketplace.search(&CapabilityQuery {
//!     keywords: vec!["scraping".into()],
//!     ..Default::default()
//! });
//!
//! let bid = marketplace.place_bid(BidRequest {
//!     listing_id: matches[0].listing_id.clone(),
//!     capability_name: "web-scraping".into(),
//!     max_price: 0.02,
//!     requester_id: "agent-042".into(),
//! });
//! ```

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

// =============================================================================
// CORE TYPES
// =============================================================================

/// Unique listing identifier.
pub type ListingId = String;

/// Unique bid identifier.
pub type BidId = String;

/// Unique contract identifier.
pub type ContractId = String;

/// A specific capability an agent offers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    /// Machine-readable capability name (e.g., "web-scraping", "code-review")
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// Searchable tags
    pub tags: Vec<String>,
    /// Semantic version of the capability
    pub version: String,
    /// Price per invocation (in abstract units)
    pub price: f64,
    /// Maximum concurrent invocations supported
    pub max_concurrency: usize,
    /// Average execution time in milliseconds
    pub avg_latency_ms: u64,
    /// Input schema (JSON Schema string, optional)
    pub input_schema: Option<String>,
    /// Output schema (JSON Schema string, optional)
    pub output_schema: Option<String>,
}

impl Capability {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            tags: Vec::new(),
            version: "1.0.0".into(),
            price: 0.0,
            max_concurrency: 1,
            avg_latency_ms: 1000,
            input_schema: None,
            output_schema: None,
        }
    }

    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    pub fn with_price(mut self, price: f64) -> Self {
        self.price = price;
        self
    }

    pub fn with_concurrency(mut self, max: usize) -> Self {
        self.max_concurrency = max;
        self
    }

    pub fn with_latency(mut self, ms: u64) -> Self {
        self.avg_latency_ms = ms;
        self
    }
}

/// An agent's listing in the marketplace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentListing {
    /// Unique listing ID (auto-generated if empty)
    pub listing_id: ListingId,
    /// Agent identifier
    pub agent_id: String,
    /// Display name
    pub display_name: String,
    /// Agent description
    pub description: String,
    /// Network address for direct connection
    pub endpoint: String,
    /// Supported transport protocols
    pub transports: Vec<TransportType>,
    /// Capabilities offered
    pub capabilities: Vec<Capability>,
    /// Reputation score (0.0 - 5.0)
    pub reputation: f64,
    /// Total completed contracts
    pub completed_contracts: u64,
    /// Registration timestamp (epoch ms)
    pub registered_at: u64,
    /// Last heartbeat timestamp (epoch ms)
    pub last_seen: u64,
    /// Whether the agent is currently online
    pub online: bool,
}

impl Default for AgentListing {
    fn default() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self {
            listing_id: String::new(),
            agent_id: String::new(),
            display_name: String::new(),
            description: String::new(),
            endpoint: String::new(),
            transports: vec![TransportType::Tcp],
            capabilities: Vec::new(),
            reputation: 0.0,
            completed_contracts: 0,
            registered_at: now,
            last_seen: now,
            online: true,
        }
    }
}

/// Transport types supported by a listed agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransportType {
    Tcp,
    Tls,
    WebSocket,
    Quic,
}

// =============================================================================
// DISCOVERY
// =============================================================================

/// Query parameters for capability search.
#[derive(Debug, Clone, Default)]
pub struct CapabilityQuery {
    /// Keywords to match against capability names, descriptions, and tags
    pub keywords: Vec<String>,
    /// Required tags (all must match)
    pub required_tags: Vec<String>,
    /// Minimum reputation score
    pub min_reputation: f64,
    /// Maximum price per invocation
    pub max_price: Option<f64>,
    /// Maximum acceptable latency in ms
    pub max_latency_ms: Option<u64>,
    /// Required transport type
    pub required_transport: Option<TransportType>,
    /// Only return online agents
    pub online_only: bool,
    /// Maximum results
    pub limit: usize,
}

/// A search result with relevance scoring.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub listing: AgentListing,
    pub matched_capability: String,
    /// Relevance score (higher is better)
    pub relevance: f64,
}

// =============================================================================
// BIDDING & CONTRACTS
// =============================================================================

/// A bid placed by a requester for a capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BidRequest {
    pub listing_id: ListingId,
    pub capability_name: String,
    pub requester_id: String,
    pub max_price: f64,
    /// Task description / parameters
    pub task_description: String,
    /// Deadline in epoch ms (0 = no deadline)
    pub deadline_ms: u64,
}

/// Status of a bid.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BidStatus {
    Pending,
    Accepted,
    Rejected,
    Expired,
    Completed,
    Disputed,
}

/// A recorded bid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bid {
    pub bid_id: BidId,
    pub listing_id: ListingId,
    pub capability_name: String,
    pub requester_id: String,
    pub provider_id: String,
    pub agreed_price: f64,
    pub status: BidStatus,
    pub created_at: u64,
    pub resolved_at: Option<u64>,
}

/// A completed or in-progress contract between two agents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contract {
    pub contract_id: ContractId,
    pub bid_id: BidId,
    pub provider_id: String,
    pub requester_id: String,
    pub capability_name: String,
    pub agreed_price: f64,
    pub status: ContractStatus,
    pub created_at: u64,
    pub completed_at: Option<u64>,
    /// Provider's self-reported result summary
    pub result_summary: Option<String>,
    /// Requester's rating (1-5)
    pub rating: Option<u8>,
    /// Requester's review
    pub review: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContractStatus {
    Active,
    Completed,
    Failed,
    Disputed,
    Cancelled,
}

// =============================================================================
// AUDIT LOG
// =============================================================================

/// Immutable audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: u64,
    pub event: AuditEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuditEvent {
    AgentRegistered {
        agent_id: String,
        listing_id: ListingId,
    },
    AgentDeregistered {
        agent_id: String,
    },
    CapabilityAdded {
        listing_id: ListingId,
        capability: String,
    },
    CapabilityRemoved {
        listing_id: ListingId,
        capability: String,
    },
    BidPlaced {
        bid_id: BidId,
        requester: String,
        provider: String,
    },
    BidAccepted {
        bid_id: BidId,
    },
    BidRejected {
        bid_id: BidId,
    },
    ContractCreated {
        contract_id: ContractId,
    },
    ContractCompleted {
        contract_id: ContractId,
        rating: Option<u8>,
    },
    ContractFailed {
        contract_id: ContractId,
        reason: String,
    },
    ReputationUpdated {
        agent_id: String,
        old: f64,
        new: f64,
    },
}

// =============================================================================
// MARKETPLACE
// =============================================================================

/// The central agent capability marketplace.
///
/// Thread-safe, lock-free design using `DashMap` for concurrent access.
pub struct Marketplace {
    /// All registered agent listings
    listings: DashMap<ListingId, AgentListing>,
    /// Agent ID → Listing ID index
    agent_index: DashMap<String, ListingId>,
    /// Active bids
    bids: DashMap<BidId, Bid>,
    /// Active and historical contracts
    contracts: DashMap<ContractId, Contract>,
    /// Reputation scores (agent_id → cumulative score data)
    reputations: DashMap<String, ReputationData>,
    /// Append-only audit log
    audit_log: std::sync::Mutex<Vec<AuditEntry>>,
    /// Auto-incrementing ID counters
    listing_counter: AtomicU64,
    bid_counter: AtomicU64,
    contract_counter: AtomicU64,
}

/// Accumulated reputation data for an agent.
#[derive(Debug, Clone, Default)]
struct ReputationData {
    total_rating: f64,
    rating_count: u64,
    completed: u64,
    failed: u64,
}

impl ReputationData {
    fn score(&self) -> f64 {
        if self.rating_count == 0 {
            return 0.0;
        }
        let base = self.total_rating / self.rating_count as f64;
        // Penalize high failure rates
        let reliability = if self.completed + self.failed > 0 {
            self.completed as f64 / (self.completed + self.failed) as f64
        } else {
            1.0
        };
        base * reliability
    }
}

impl Marketplace {
    /// Create a new empty marketplace.
    pub fn new() -> Self {
        Self {
            listings: DashMap::new(),
            agent_index: DashMap::new(),
            bids: DashMap::new(),
            contracts: DashMap::new(),
            reputations: DashMap::new(),
            audit_log: std::sync::Mutex::new(Vec::new()),
            listing_counter: AtomicU64::new(1),
            bid_counter: AtomicU64::new(1),
            contract_counter: AtomicU64::new(1),
        }
    }

    /// Create a marketplace wrapped in an `Arc` for shared ownership.
    pub fn shared() -> Arc<Self> {
        Arc::new(Self::new())
    }

    // =========================================================================
    // REGISTRATION
    // =========================================================================

    /// Register an agent listing. Returns the assigned listing ID.
    pub fn register(&self, mut listing: AgentListing) -> ListingId {
        let id = format!(
            "listing-{}",
            self.listing_counter.fetch_add(1, Ordering::Relaxed)
        );
        listing.listing_id = id.clone();
        listing.registered_at = now_ms();
        listing.last_seen = now_ms();

        self.agent_index
            .insert(listing.agent_id.clone(), id.clone());
        self.audit(AuditEvent::AgentRegistered {
            agent_id: listing.agent_id.clone(),
            listing_id: id.clone(),
        });
        self.listings.insert(id.clone(), listing);
        id
    }

    /// Remove an agent's listing.
    pub fn deregister(&self, agent_id: &str) -> bool {
        if let Some((_, lid)) = self.agent_index.remove(agent_id) {
            self.listings.remove(&lid);
            self.audit(AuditEvent::AgentDeregistered {
                agent_id: agent_id.to_string(),
            });
            true
        } else {
            false
        }
    }

    /// Update an agent's heartbeat (mark as online).
    pub fn heartbeat(&self, agent_id: &str) {
        if let Some(lid) = self.agent_index.get(agent_id) {
            if let Some(mut listing) = self.listings.get_mut(lid.value()) {
                listing.last_seen = now_ms();
                listing.online = true;
            }
        }
    }

    /// Add a capability to an existing listing.
    pub fn add_capability(&self, agent_id: &str, capability: Capability) -> bool {
        if let Some(lid) = self.agent_index.get(agent_id) {
            if let Some(mut listing) = self.listings.get_mut(lid.value()) {
                let name = capability.name.clone();
                listing.capabilities.push(capability);
                self.audit(AuditEvent::CapabilityAdded {
                    listing_id: lid.value().clone(),
                    capability: name,
                });
                return true;
            }
        }
        false
    }

    /// Remove a capability from a listing.
    pub fn remove_capability(&self, agent_id: &str, capability_name: &str) -> bool {
        if let Some(lid) = self.agent_index.get(agent_id) {
            if let Some(mut listing) = self.listings.get_mut(lid.value()) {
                let before = listing.capabilities.len();
                listing.capabilities.retain(|c| c.name != capability_name);
                if listing.capabilities.len() < before {
                    self.audit(AuditEvent::CapabilityRemoved {
                        listing_id: lid.value().clone(),
                        capability: capability_name.to_string(),
                    });
                    return true;
                }
            }
        }
        false
    }

    /// Get a listing by agent ID.
    pub fn get_listing(&self, agent_id: &str) -> Option<AgentListing> {
        self.agent_index
            .get(agent_id)
            .and_then(|lid| self.listings.get(lid.value()).map(|l| l.clone()))
    }

    /// Get total number of registered agents.
    pub fn agent_count(&self) -> usize {
        self.listings.len()
    }

    // =========================================================================
    // DISCOVERY
    // =========================================================================

    /// Search for agents matching a capability query.
    pub fn search(&self, query: &CapabilityQuery) -> Vec<SearchResult> {
        let limit = if query.limit == 0 { 50 } else { query.limit };
        let mut results = Vec::new();

        for entry in self.listings.iter() {
            let listing = entry.value();

            // Online filter
            if query.online_only && !listing.online {
                continue;
            }

            // Reputation filter
            if listing.reputation < query.min_reputation {
                continue;
            }

            // Transport filter
            if let Some(ref required) = query.required_transport {
                if !listing.transports.contains(required) {
                    continue;
                }
            }

            // Match against capabilities
            for cap in &listing.capabilities {
                // Price filter
                if let Some(max_price) = query.max_price {
                    if cap.price > max_price {
                        continue;
                    }
                }

                // Latency filter
                if let Some(max_latency) = query.max_latency_ms {
                    if cap.avg_latency_ms > max_latency {
                        continue;
                    }
                }

                // Required tags filter
                if !query.required_tags.is_empty()
                    && !query
                        .required_tags
                        .iter()
                        .all(|t| cap.tags.iter().any(|ct| ct.eq_ignore_ascii_case(t)))
                {
                    continue;
                }

                // Keyword relevance scoring
                let relevance = compute_relevance(cap, &query.keywords, listing.reputation);
                if relevance > 0.0 || query.keywords.is_empty() {
                    results.push(SearchResult {
                        listing: listing.clone(),
                        matched_capability: cap.name.clone(),
                        relevance,
                    });
                }
            }
        }

        // Sort by relevance descending
        results.sort_by(|a, b| {
            b.relevance
                .partial_cmp(&a.relevance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);
        results
    }

    /// Find agents that can fulfill ALL of the given capability names.
    pub fn find_composite(&self, capabilities: &[&str]) -> Vec<AgentListing> {
        self.listings
            .iter()
            .filter(|entry| {
                let listing = entry.value();
                capabilities
                    .iter()
                    .all(|required| listing.capabilities.iter().any(|c| c.name == *required))
            })
            .map(|entry| entry.value().clone())
            .collect()
    }

    // =========================================================================
    // BIDDING
    // =========================================================================

    /// Place a bid on a listing's capability.
    pub fn place_bid(&self, request: BidRequest) -> Result<BidId, MarketplaceError> {
        let listing = self
            .listings
            .get(&request.listing_id)
            .ok_or(MarketplaceError::ListingNotFound)?;

        // Verify capability exists
        let cap = listing
            .capabilities
            .iter()
            .find(|c| c.name == request.capability_name)
            .ok_or(MarketplaceError::CapabilityNotFound)?;

        // Check price
        if request.max_price < cap.price {
            return Err(MarketplaceError::PriceTooLow {
                offered: request.max_price,
                minimum: cap.price,
            });
        }

        let bid_id = format!("bid-{}", self.bid_counter.fetch_add(1, Ordering::Relaxed));
        let provider_id = listing.agent_id.clone();

        let bid = Bid {
            bid_id: bid_id.clone(),
            listing_id: request.listing_id,
            capability_name: request.capability_name,
            requester_id: request.requester_id.clone(),
            provider_id: provider_id.clone(),
            agreed_price: cap.price,
            status: BidStatus::Pending,
            created_at: now_ms(),
            resolved_at: None,
        };

        self.audit(AuditEvent::BidPlaced {
            bid_id: bid_id.clone(),
            requester: request.requester_id,
            provider: provider_id,
        });

        self.bids.insert(bid_id.clone(), bid);
        Ok(bid_id)
    }

    /// Accept a pending bid, creating a contract.
    pub fn accept_bid(&self, bid_id: &str) -> Result<ContractId, MarketplaceError> {
        let mut bid = self
            .bids
            .get_mut(bid_id)
            .ok_or(MarketplaceError::BidNotFound)?;

        if bid.status != BidStatus::Pending {
            return Err(MarketplaceError::InvalidBidState);
        }

        bid.status = BidStatus::Accepted;
        bid.resolved_at = Some(now_ms());

        let contract_id = format!(
            "contract-{}",
            self.contract_counter.fetch_add(1, Ordering::Relaxed)
        );
        let contract = Contract {
            contract_id: contract_id.clone(),
            bid_id: bid_id.to_string(),
            provider_id: bid.provider_id.clone(),
            requester_id: bid.requester_id.clone(),
            capability_name: bid.capability_name.clone(),
            agreed_price: bid.agreed_price,
            status: ContractStatus::Active,
            created_at: now_ms(),
            completed_at: None,
            result_summary: None,
            rating: None,
            review: None,
        };

        self.audit(AuditEvent::BidAccepted {
            bid_id: bid_id.to_string(),
        });
        self.audit(AuditEvent::ContractCreated {
            contract_id: contract_id.clone(),
        });

        self.contracts.insert(contract_id.clone(), contract);
        Ok(contract_id)
    }

    /// Reject a pending bid.
    pub fn reject_bid(&self, bid_id: &str) -> Result<(), MarketplaceError> {
        let mut bid = self
            .bids
            .get_mut(bid_id)
            .ok_or(MarketplaceError::BidNotFound)?;

        if bid.status != BidStatus::Pending {
            return Err(MarketplaceError::InvalidBidState);
        }

        bid.status = BidStatus::Rejected;
        bid.resolved_at = Some(now_ms());
        self.audit(AuditEvent::BidRejected {
            bid_id: bid_id.to_string(),
        });
        Ok(())
    }

    // =========================================================================
    // CONTRACTS
    // =========================================================================

    /// Mark a contract as completed with an optional result summary.
    pub fn complete_contract(
        &self,
        contract_id: &str,
        result_summary: Option<String>,
    ) -> Result<(), MarketplaceError> {
        let mut contract = self
            .contracts
            .get_mut(contract_id)
            .ok_or(MarketplaceError::ContractNotFound)?;

        if contract.status != ContractStatus::Active {
            return Err(MarketplaceError::InvalidContractState);
        }

        contract.status = ContractStatus::Completed;
        contract.completed_at = Some(now_ms());
        contract.result_summary = result_summary;

        // Update reputation data
        self.reputations
            .entry(contract.provider_id.clone())
            .or_default()
            .completed += 1;

        // Update listing's completed count
        if let Some(lid) = self.agent_index.get(&contract.provider_id) {
            if let Some(mut listing) = self.listings.get_mut(lid.value()) {
                listing.completed_contracts += 1;
            }
        }

        self.audit(AuditEvent::ContractCompleted {
            contract_id: contract_id.to_string(),
            rating: None,
        });

        Ok(())
    }

    /// Mark a contract as failed.
    pub fn fail_contract(&self, contract_id: &str, reason: &str) -> Result<(), MarketplaceError> {
        let mut contract = self
            .contracts
            .get_mut(contract_id)
            .ok_or(MarketplaceError::ContractNotFound)?;

        if contract.status != ContractStatus::Active {
            return Err(MarketplaceError::InvalidContractState);
        }

        contract.status = ContractStatus::Failed;
        contract.completed_at = Some(now_ms());

        self.reputations
            .entry(contract.provider_id.clone())
            .or_default()
            .failed += 1;

        self.audit(AuditEvent::ContractFailed {
            contract_id: contract_id.to_string(),
            reason: reason.to_string(),
        });

        Ok(())
    }

    /// Rate a completed contract.
    pub fn rate_contract(
        &self,
        contract_id: &str,
        rating: u8,
        review: Option<String>,
    ) -> Result<(), MarketplaceError> {
        if !(1..=5).contains(&rating) {
            return Err(MarketplaceError::InvalidRating);
        }

        let mut contract = self
            .contracts
            .get_mut(contract_id)
            .ok_or(MarketplaceError::ContractNotFound)?;

        if contract.status != ContractStatus::Completed {
            return Err(MarketplaceError::InvalidContractState);
        }

        contract.rating = Some(rating);
        contract.review = review;

        // Update reputation
        let provider_id = contract.provider_id.clone();
        let mut rep = self.reputations.entry(provider_id.clone()).or_default();
        let old_score = rep.score();
        rep.total_rating += rating as f64;
        rep.rating_count += 1;
        let new_score = rep.score();
        drop(rep);

        // Propagate to listing
        if let Some(lid) = self.agent_index.get(&provider_id) {
            if let Some(mut listing) = self.listings.get_mut(lid.value()) {
                listing.reputation = new_score;
            }
        }

        self.audit(AuditEvent::ReputationUpdated {
            agent_id: provider_id,
            old: old_score,
            new: new_score,
        });

        Ok(())
    }

    /// Get a contract by ID.
    pub fn get_contract(&self, contract_id: &str) -> Option<Contract> {
        self.contracts.get(contract_id).map(|c| c.clone())
    }

    /// Get all contracts for an agent (as provider or requester).
    pub fn agent_contracts(&self, agent_id: &str) -> Vec<Contract> {
        self.contracts
            .iter()
            .filter(|c| c.provider_id == agent_id || c.requester_id == agent_id)
            .map(|c| c.value().clone())
            .collect()
    }

    // =========================================================================
    // REPUTATION
    // =========================================================================

    /// Get an agent's computed reputation score.
    pub fn reputation(&self, agent_id: &str) -> f64 {
        self.reputations
            .get(agent_id)
            .map(|r| r.score())
            .unwrap_or(0.0)
    }

    // =========================================================================
    // STATISTICS
    // =========================================================================

    /// Get marketplace statistics.
    pub fn stats(&self) -> MarketplaceStats {
        let total_capabilities: usize = self.listings.iter().map(|l| l.capabilities.len()).sum();
        let online_agents = self.listings.iter().filter(|l| l.online).count();

        MarketplaceStats {
            total_agents: self.listings.len(),
            online_agents,
            total_capabilities,
            active_bids: self
                .bids
                .iter()
                .filter(|b| b.status == BidStatus::Pending)
                .count(),
            active_contracts: self
                .contracts
                .iter()
                .filter(|c| c.status == ContractStatus::Active)
                .count(),
            completed_contracts: self
                .contracts
                .iter()
                .filter(|c| c.status == ContractStatus::Completed)
                .count(),
            audit_log_size: self.audit_log.lock().unwrap().len(),
        }
    }

    /// Get the audit log (clone).
    pub fn audit_log(&self) -> Vec<AuditEntry> {
        self.audit_log.lock().unwrap().clone()
    }

    // =========================================================================
    // INTERNAL
    // =========================================================================

    fn audit(&self, event: AuditEvent) {
        if let Ok(mut log) = self.audit_log.lock() {
            log.push(AuditEntry {
                timestamp: now_ms(),
                event,
            });
        }
    }
}

impl Default for Marketplace {
    fn default() -> Self {
        Self::new()
    }
}

/// Marketplace statistics snapshot.
#[derive(Debug, Clone)]
pub struct MarketplaceStats {
    pub total_agents: usize,
    pub online_agents: usize,
    pub total_capabilities: usize,
    pub active_bids: usize,
    pub active_contracts: usize,
    pub completed_contracts: usize,
    pub audit_log_size: usize,
}

/// Marketplace errors.
#[derive(Debug, Clone, thiserror::Error)]
pub enum MarketplaceError {
    #[error("Listing not found")]
    ListingNotFound,
    #[error("Capability not found")]
    CapabilityNotFound,
    #[error("Bid not found")]
    BidNotFound,
    #[error("Contract not found")]
    ContractNotFound,
    #[error("Price too low: offered {offered}, minimum {minimum}")]
    PriceTooLow { offered: f64, minimum: f64 },
    #[error("Invalid bid state for this operation")]
    InvalidBidState,
    #[error("Invalid contract state for this operation")]
    InvalidContractState,
    #[error("Rating must be 1-5")]
    InvalidRating,
}

// =============================================================================
// HELPERS
// =============================================================================

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Compute relevance score for a capability against keywords.
fn compute_relevance(cap: &Capability, keywords: &[String], reputation: f64) -> f64 {
    if keywords.is_empty() {
        // No keywords = base relevance from reputation
        return 1.0 + reputation;
    }

    let mut score = 0.0;

    for kw in keywords {
        let kw_lower = kw.to_lowercase();

        // Exact name match = high score
        if cap.name.to_lowercase() == kw_lower {
            score += 10.0;
        } else if cap.name.to_lowercase().contains(&kw_lower) {
            score += 5.0;
        }

        // Description match
        if cap.description.to_lowercase().contains(&kw_lower) {
            score += 2.0;
        }

        // Tag match
        for tag in &cap.tags {
            if tag.to_lowercase() == kw_lower {
                score += 4.0;
            } else if tag.to_lowercase().contains(&kw_lower) {
                score += 2.0;
            }
        }
    }

    // Boost by reputation
    score * (1.0 + reputation * 0.1)
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_marketplace() -> Marketplace {
        let m = Marketplace::new();

        m.register(AgentListing {
            agent_id: "scraper-001".into(),
            display_name: "Web Scraper Pro".into(),
            description: "High-performance web scraping agent".into(),
            endpoint: "tcp://10.0.0.1:3000".into(),
            transports: vec![TransportType::Tcp, TransportType::WebSocket],
            capabilities: vec![
                Capability::new("web-scraping", "Extract structured data from web pages")
                    .with_tag("html")
                    .with_tag("parsing")
                    .with_price(0.01)
                    .with_latency(500),
                Capability::new("screenshot", "Capture page screenshots")
                    .with_tag("visual")
                    .with_price(0.02)
                    .with_latency(2000),
            ],
            ..Default::default()
        });

        m.register(AgentListing {
            agent_id: "analyzer-002".into(),
            display_name: "Data Analyzer".into(),
            description: "ML-powered data analysis agent".into(),
            endpoint: "tcp://10.0.0.2:3000".into(),
            transports: vec![TransportType::Tcp, TransportType::Quic],
            capabilities: vec![
                Capability::new("sentiment-analysis", "Analyze text sentiment")
                    .with_tag("nlp")
                    .with_tag("ml")
                    .with_price(0.05)
                    .with_latency(200),
                Capability::new("summarization", "Summarize long documents")
                    .with_tag("nlp")
                    .with_price(0.10)
                    .with_latency(3000),
            ],
            ..Default::default()
        });

        m
    }

    #[test]
    fn test_register_and_lookup() {
        let m = test_marketplace();
        assert_eq!(m.agent_count(), 2);

        let listing = m.get_listing("scraper-001").unwrap();
        assert_eq!(listing.capabilities.len(), 2);
        assert_eq!(listing.display_name, "Web Scraper Pro");
    }

    #[test]
    fn test_deregister() {
        let m = test_marketplace();
        assert!(m.deregister("scraper-001"));
        assert_eq!(m.agent_count(), 1);
        assert!(m.get_listing("scraper-001").is_none());
    }

    #[test]
    fn test_search_by_keyword() {
        let m = test_marketplace();
        let results = m.search(&CapabilityQuery {
            keywords: vec!["scraping".into()],
            ..Default::default()
        });
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].matched_capability, "web-scraping");
    }

    #[test]
    fn test_search_by_tag() {
        let m = test_marketplace();
        let results = m.search(&CapabilityQuery {
            required_tags: vec!["nlp".into()],
            ..Default::default()
        });
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_search_by_transport() {
        let m = test_marketplace();
        let results = m.search(&CapabilityQuery {
            required_transport: Some(TransportType::WebSocket),
            ..Default::default()
        });
        // Only scraper-001 supports WebSocket
        assert!(results.iter().all(|r| r.listing.agent_id == "scraper-001"));
    }

    #[test]
    fn test_search_max_price() {
        let m = test_marketplace();
        let results = m.search(&CapabilityQuery {
            max_price: Some(0.02),
            ..Default::default()
        });
        // Should exclude sentiment-analysis (0.05) and summarization (0.10)
        assert!(results.iter().all(|r| {
            let cap = r
                .listing
                .capabilities
                .iter()
                .find(|c| c.name == r.matched_capability)
                .unwrap();
            cap.price <= 0.02
        }));
    }

    #[test]
    fn test_bid_workflow() {
        let m = test_marketplace();

        let listing = m.get_listing("scraper-001").unwrap();
        let bid_id = m
            .place_bid(BidRequest {
                listing_id: listing.listing_id.clone(),
                capability_name: "web-scraping".into(),
                requester_id: "requester-x".into(),
                max_price: 0.05,
                task_description: "Scrape 100 pages".into(),
                deadline_ms: 0,
            })
            .unwrap();

        // Accept the bid → creates a contract
        let contract_id = m.accept_bid(&bid_id).unwrap();

        // Complete the contract
        m.complete_contract(&contract_id, Some("Scraped 100 pages successfully".into()))
            .unwrap();

        // Rate it
        m.rate_contract(&contract_id, 5, Some("Excellent work!".into()))
            .unwrap();

        // Check reputation was updated
        let rep = m.reputation("scraper-001");
        assert!(rep > 0.0);

        let contract = m.get_contract(&contract_id).unwrap();
        assert_eq!(contract.status, ContractStatus::Completed);
        assert_eq!(contract.rating, Some(5));
    }

    #[test]
    fn test_bid_price_too_low() {
        let m = test_marketplace();
        let listing = m.get_listing("scraper-001").unwrap();

        let result = m.place_bid(BidRequest {
            listing_id: listing.listing_id,
            capability_name: "web-scraping".into(),
            requester_id: "cheapskate".into(),
            max_price: 0.001, // Below minimum
            task_description: "Free scraping please".into(),
            deadline_ms: 0,
        });

        assert!(matches!(result, Err(MarketplaceError::PriceTooLow { .. })));
    }

    #[test]
    fn test_reject_bid() {
        let m = test_marketplace();
        let listing = m.get_listing("scraper-001").unwrap();

        let bid_id = m
            .place_bid(BidRequest {
                listing_id: listing.listing_id,
                capability_name: "web-scraping".into(),
                requester_id: "requester-y".into(),
                max_price: 0.05,
                task_description: "Scrape stuff".into(),
                deadline_ms: 0,
            })
            .unwrap();

        m.reject_bid(&bid_id).unwrap();

        // Can't accept after rejection
        assert!(m.accept_bid(&bid_id).is_err());
    }

    #[test]
    fn test_find_composite() {
        let m = test_marketplace();
        // scraper-001 has both web-scraping and screenshot
        let agents = m.find_composite(&["web-scraping", "screenshot"]);
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].agent_id, "scraper-001");

        // No agent has both scraping and sentiment
        let agents = m.find_composite(&["web-scraping", "sentiment-analysis"]);
        assert!(agents.is_empty());
    }

    #[test]
    fn test_add_remove_capability() {
        let m = test_marketplace();

        m.add_capability(
            "scraper-001",
            Capability::new("pdf-extraction", "Extract text from PDFs").with_price(0.03),
        );

        let listing = m.get_listing("scraper-001").unwrap();
        assert_eq!(listing.capabilities.len(), 3);

        m.remove_capability("scraper-001", "pdf-extraction");
        let listing = m.get_listing("scraper-001").unwrap();
        assert_eq!(listing.capabilities.len(), 2);
    }

    #[test]
    fn test_stats() {
        let m = test_marketplace();
        let stats = m.stats();
        assert_eq!(stats.total_agents, 2);
        assert_eq!(stats.total_capabilities, 4);
        assert_eq!(stats.active_bids, 0);
    }

    #[test]
    fn test_audit_log() {
        let m = test_marketplace();
        let log = m.audit_log();
        // Two registrations
        assert_eq!(log.len(), 2);
        assert!(matches!(&log[0].event, AuditEvent::AgentRegistered { .. }));
    }

    #[test]
    fn test_contract_failure() {
        let m = test_marketplace();
        let listing = m.get_listing("analyzer-002").unwrap();

        let bid_id = m
            .place_bid(BidRequest {
                listing_id: listing.listing_id,
                capability_name: "sentiment-analysis".into(),
                requester_id: "requester-z".into(),
                max_price: 1.0,
                task_description: "Analyze tweets".into(),
                deadline_ms: 0,
            })
            .unwrap();

        let contract_id = m.accept_bid(&bid_id).unwrap();
        m.fail_contract(&contract_id, "Timeout exceeded").unwrap();

        let contract = m.get_contract(&contract_id).unwrap();
        assert_eq!(contract.status, ContractStatus::Failed);
    }
}
