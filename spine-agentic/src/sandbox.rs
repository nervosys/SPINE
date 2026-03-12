// =============================================================================
// SPINE Agent WASM Sandboxing
// =============================================================================
//
// Capability-based security model for agent WASM execution.
// Each agent gets a sandbox with explicit capability grants controlling
// which host functions it can call.
//
// =============================================================================

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::AgentId;

// =============================================================================
// CAPABILITY DEFINITIONS
// =============================================================================

/// A capability that can be granted to an agent's WASM sandbox.
///
/// Capabilities are fine-grained permissions controlling access to
/// host functions exposed to the WASM runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WasmCapability {
    // --- DOM Operations ---
    /// Create elements and manipulate the virtual DOM.
    DomWrite,
    /// Read element attributes and text.
    DomRead,

    // --- Network ---
    /// Navigate to URLs (Navigate action).
    NetworkNavigate,
    /// Perform search queries (Search action).
    NetworkSearch,

    // --- State ---
    /// Declare and read state variables.
    StateRead,
    /// Update state variables.
    StateWrite,

    // --- Events ---
    /// Emit events to the host.
    EventEmit,

    // --- Neural / Crypto ---
    /// Stream latent vectors to the protocol layer.
    LatentStream,
    /// Trigger protocol morphology changes.
    ProtocolMorph,
    /// Inject decoy messages.
    DecoyInject,

    // --- Knowledge ---
    /// Store knowledge entries.
    KnowledgeWrite,
    /// Query the knowledge base.
    KnowledgeRead,

    // --- System ---
    /// Access filesystem (via host bridge).
    FileSystem,
    /// Make HTTP requests (via host bridge).
    HttpClient,
    /// Spawn child agents.
    AgentSpawn,
    /// Send messages to other agents.
    AgentMessage,
}

impl WasmCapability {
    /// Returns the set of capabilities considered safe for untrusted code.
    pub fn safe_defaults() -> HashSet<WasmCapability> {
        [
            WasmCapability::DomRead,
            WasmCapability::DomWrite,
            WasmCapability::StateRead,
            WasmCapability::StateWrite,
            WasmCapability::EventEmit,
            WasmCapability::KnowledgeRead,
        ]
        .into_iter()
        .collect()
    }

    /// Returns all capabilities (for fully trusted agents).
    pub fn all() -> HashSet<WasmCapability> {
        [
            WasmCapability::DomWrite,
            WasmCapability::DomRead,
            WasmCapability::NetworkNavigate,
            WasmCapability::NetworkSearch,
            WasmCapability::StateRead,
            WasmCapability::StateWrite,
            WasmCapability::EventEmit,
            WasmCapability::LatentStream,
            WasmCapability::ProtocolMorph,
            WasmCapability::DecoyInject,
            WasmCapability::KnowledgeWrite,
            WasmCapability::KnowledgeRead,
            WasmCapability::FileSystem,
            WasmCapability::HttpClient,
            WasmCapability::AgentSpawn,
            WasmCapability::AgentMessage,
        ]
        .into_iter()
        .collect()
    }
}

// =============================================================================
// RESOURCE LIMITS
// =============================================================================

/// Resource limits enforced on a WASM sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxLimits {
    /// Maximum WASM linear memory in bytes (default: 16 MiB).
    pub max_memory_bytes: usize,
    /// Maximum number of WASM instructions before execution is halted.
    pub max_instructions: u64,
    /// Maximum execution wall-clock time in milliseconds.
    pub max_execution_ms: u64,
    /// Maximum number of elements that can be created.
    pub max_elements: u32,
    /// Maximum number of events that can be emitted.
    pub max_events: u32,
    /// Maximum number of latent vectors that can be streamed.
    pub max_latent_streams: u32,
    /// Maximum number of knowledge operations per execution.
    pub max_knowledge_ops: u32,
    /// Maximum size of a single state value in bytes.
    pub max_state_value_bytes: usize,
}

impl Default for SandboxLimits {
    fn default() -> Self {
        Self {
            max_memory_bytes: 16 * 1024 * 1024, // 16 MiB
            max_instructions: 10_000_000,
            max_execution_ms: 5_000,
            max_elements: 10_000,
            max_events: 1_000,
            max_latent_streams: 100,
            max_knowledge_ops: 500,
            max_state_value_bytes: 1024 * 1024, // 1 MiB
        }
    }
}

impl SandboxLimits {
    /// Restrictive limits for untrusted agents.
    pub fn restricted() -> Self {
        Self {
            max_memory_bytes: 4 * 1024 * 1024, // 4 MiB
            max_instructions: 1_000_000,
            max_execution_ms: 1_000,
            max_elements: 100,
            max_events: 50,
            max_latent_streams: 10,
            max_knowledge_ops: 20,
            max_state_value_bytes: 64 * 1024, // 64 KiB
        }
    }
}

// =============================================================================
// SANDBOX POLICY
// =============================================================================

/// Complete security policy for an agent's WASM execution sandbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxPolicy {
    /// Unique identifier for this policy.
    pub id: Uuid,
    /// Human-readable name.
    pub name: String,
    /// Granted capabilities.
    pub capabilities: HashSet<WasmCapability>,
    /// Resource limits.
    pub limits: SandboxLimits,
    /// Allowed URL patterns for network access (glob-style).
    pub allowed_url_patterns: Vec<String>,
    /// Blocked URL patterns (takes precedence over allowed).
    pub blocked_url_patterns: Vec<String>,
    /// Whether the sandbox can access the agent's own knowledge only.
    pub knowledge_isolation: bool,
    /// Created timestamp.
    pub created_at: DateTime<Utc>,
}

impl SandboxPolicy {
    /// Create a default safe policy for untrusted code.
    pub fn untrusted(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            capabilities: WasmCapability::safe_defaults(),
            limits: SandboxLimits::restricted(),
            allowed_url_patterns: vec![],
            blocked_url_patterns: vec![],
            knowledge_isolation: true,
            created_at: Utc::now(),
        }
    }

    /// Create a permissive policy for trusted agents.
    pub fn trusted(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            capabilities: WasmCapability::all(),
            limits: SandboxLimits::default(),
            allowed_url_patterns: vec!["*".to_string()],
            blocked_url_patterns: vec![],
            knowledge_isolation: false,
            created_at: Utc::now(),
        }
    }

    /// Check if a capability is granted.
    pub fn has_capability(&self, cap: WasmCapability) -> bool {
        self.capabilities.contains(&cap)
    }

    /// Check whether the given URL is permitted by this policy.
    pub fn is_url_allowed(&self, url: &str) -> bool {
        // Blocked patterns take precedence
        for pattern in &self.blocked_url_patterns {
            if glob_match(pattern, url) {
                return false;
            }
        }
        if self.allowed_url_patterns.is_empty() {
            return false;
        }
        for pattern in &self.allowed_url_patterns {
            if glob_match(pattern, url) {
                return true;
            }
        }
        false
    }
}

/// Simple glob matching: `*` matches everything, `*.example.com` matches suffix.
fn glob_match(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    match (pattern.strip_prefix('*'), pattern.strip_suffix('*')) {
        (Some(mid), _) if mid.ends_with('*') => {
            // *something* — contains match
            let inner = &mid[..mid.len() - 1];
            return value.contains(inner);
        }
        (Some(suffix), _) => return value.ends_with(suffix),
        (_, Some(prefix)) => return value.starts_with(prefix),
        _ => {}
    }
    pattern == value
}

// =============================================================================
// SANDBOX INSTANCE
// =============================================================================

/// Runtime state of an active sandbox, tracking resource consumption.
#[derive(Debug, Clone)]
pub struct SandboxInstance {
    pub agent_id: AgentId,
    pub policy: SandboxPolicy,
    pub instructions_used: u64,
    pub memory_used_bytes: usize,
    pub elements_created: u32,
    pub events_emitted: u32,
    pub latent_streams_sent: u32,
    pub knowledge_ops: u32,
    pub started_at: DateTime<Utc>,
    pub violations: Vec<SandboxViolation>,
}

/// A recorded security violation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxViolation {
    pub timestamp: DateTime<Utc>,
    pub violation_type: ViolationType,
    pub details: String,
}

/// Types of sandbox violations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ViolationType {
    CapabilityDenied,
    ResourceLimitExceeded,
    UrlBlocked,
    KnowledgeIsolationBreached,
}

impl SandboxInstance {
    /// Create a new sandbox instance for an agent.
    pub fn new(agent_id: AgentId, policy: SandboxPolicy) -> Self {
        Self {
            agent_id,
            policy,
            instructions_used: 0,
            memory_used_bytes: 0,
            elements_created: 0,
            events_emitted: 0,
            latent_streams_sent: 0,
            knowledge_ops: 0,
            started_at: Utc::now(),
            violations: Vec::new(),
        }
    }

    /// Check if a capability is permitted. Records a violation if denied.
    pub fn check_capability(&mut self, cap: WasmCapability) -> Result<(), SandboxViolation> {
        if self.policy.has_capability(cap) {
            Ok(())
        } else {
            let violation = SandboxViolation {
                timestamp: Utc::now(),
                violation_type: ViolationType::CapabilityDenied,
                details: format!("Capability {:?} not granted", cap),
            };
            self.violations.push(violation.clone());
            Err(violation)
        }
    }

    /// Check and consume an instruction budget.
    pub fn consume_instructions(&mut self, count: u64) -> Result<(), SandboxViolation> {
        self.instructions_used += count;
        if self.instructions_used > self.policy.limits.max_instructions {
            let violation = SandboxViolation {
                timestamp: Utc::now(),
                violation_type: ViolationType::ResourceLimitExceeded,
                details: format!(
                    "Instruction limit exceeded: {} > {}",
                    self.instructions_used, self.policy.limits.max_instructions
                ),
            };
            self.violations.push(violation.clone());
            Err(violation)
        } else {
            Ok(())
        }
    }

    /// Track element creation against limits.
    pub fn track_element(&mut self) -> Result<(), SandboxViolation> {
        self.elements_created += 1;
        if self.elements_created > self.policy.limits.max_elements {
            let violation = SandboxViolation {
                timestamp: Utc::now(),
                violation_type: ViolationType::ResourceLimitExceeded,
                details: format!(
                    "Element limit exceeded: {} > {}",
                    self.elements_created, self.policy.limits.max_elements
                ),
            };
            self.violations.push(violation.clone());
            Err(violation)
        } else {
            Ok(())
        }
    }

    /// Track event emission against limits.
    pub fn track_event(&mut self) -> Result<(), SandboxViolation> {
        self.events_emitted += 1;
        if self.events_emitted > self.policy.limits.max_events {
            let violation = SandboxViolation {
                timestamp: Utc::now(),
                violation_type: ViolationType::ResourceLimitExceeded,
                details: "Event limit exceeded".to_string(),
            };
            self.violations.push(violation.clone());
            Err(violation)
        } else {
            Ok(())
        }
    }

    /// Track latent stream sending against limits.
    pub fn track_latent_stream(&mut self) -> Result<(), SandboxViolation> {
        self.latent_streams_sent += 1;
        if self.latent_streams_sent > self.policy.limits.max_latent_streams {
            let violation = SandboxViolation {
                timestamp: Utc::now(),
                violation_type: ViolationType::ResourceLimitExceeded,
                details: "Latent stream limit exceeded".to_string(),
            };
            self.violations.push(violation.clone());
            Err(violation)
        } else {
            Ok(())
        }
    }

    /// Track knowledge operations against limits.
    pub fn track_knowledge_op(&mut self) -> Result<(), SandboxViolation> {
        self.knowledge_ops += 1;
        if self.knowledge_ops > self.policy.limits.max_knowledge_ops {
            let violation = SandboxViolation {
                timestamp: Utc::now(),
                violation_type: ViolationType::ResourceLimitExceeded,
                details: "Knowledge operation limit exceeded".to_string(),
            };
            self.violations.push(violation.clone());
            Err(violation)
        } else {
            Ok(())
        }
    }

    /// Check if a URL is permitted by this sandbox.
    pub fn check_url(&mut self, url: &str) -> Result<(), SandboxViolation> {
        if self.policy.is_url_allowed(url) {
            Ok(())
        } else {
            let violation = SandboxViolation {
                timestamp: Utc::now(),
                violation_type: ViolationType::UrlBlocked,
                details: format!("URL not permitted: {}", url),
            };
            self.violations.push(violation.clone());
            Err(violation)
        }
    }

    /// Whether the sandbox has recorded any violations.
    pub fn has_violations(&self) -> bool {
        !self.violations.is_empty()
    }

    /// Get a summary of resource usage.
    pub fn usage_summary(&self) -> SandboxUsage {
        SandboxUsage {
            instructions_used: self.instructions_used,
            instructions_limit: self.policy.limits.max_instructions,
            memory_used_bytes: self.memory_used_bytes,
            memory_limit_bytes: self.policy.limits.max_memory_bytes,
            elements_created: self.elements_created,
            events_emitted: self.events_emitted,
            violations_count: self.violations.len(),
        }
    }
}

/// Summary of sandbox resource usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxUsage {
    pub instructions_used: u64,
    pub instructions_limit: u64,
    pub memory_used_bytes: usize,
    pub memory_limit_bytes: usize,
    pub elements_created: u32,
    pub events_emitted: u32,
    pub violations_count: usize,
}

// =============================================================================
// SANDBOX REGISTRY
// =============================================================================

/// Registry of sandbox policies and active instances.
pub struct SandboxRegistry {
    policies: HashMap<String, SandboxPolicy>,
    active: HashMap<AgentId, SandboxInstance>,
}

impl Default for SandboxRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SandboxRegistry {
    pub fn new() -> Self {
        let mut policies = HashMap::new();
        policies.insert("untrusted".to_string(), SandboxPolicy::untrusted("untrusted"));
        policies.insert("trusted".to_string(), SandboxPolicy::trusted("trusted"));
        Self {
            policies,
            active: HashMap::new(),
        }
    }

    /// Register a named policy.
    pub fn register_policy(&mut self, name: impl Into<String>, policy: SandboxPolicy) {
        self.policies.insert(name.into(), policy);
    }

    /// Get a policy by name.
    pub fn get_policy(&self, name: &str) -> Option<&SandboxPolicy> {
        self.policies.get(name)
    }

    /// Create a sandbox instance for an agent with the named policy.
    pub fn create_sandbox(
        &mut self,
        agent_id: AgentId,
        policy_name: &str,
    ) -> Result<&SandboxInstance, String> {
        let policy = self
            .policies
            .get(policy_name)
            .ok_or_else(|| format!("Policy '{}' not found", policy_name))?
            .clone();
        let instance = SandboxInstance::new(agent_id, policy);
        self.active.insert(agent_id, instance);
        Ok(self.active.get(&agent_id).unwrap())
    }

    /// Get a mutable reference to an active sandbox.
    pub fn get_sandbox_mut(&mut self, agent_id: &AgentId) -> Option<&mut SandboxInstance> {
        self.active.get_mut(agent_id)
    }

    /// Remove and return a sandbox instance (agent finished execution).
    pub fn remove_sandbox(&mut self, agent_id: &AgentId) -> Option<SandboxInstance> {
        self.active.remove(agent_id)
    }

    /// List all active sandboxes.
    pub fn active_count(&self) -> usize {
        self.active.len()
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_defaults() {
        let caps = WasmCapability::safe_defaults();
        assert!(caps.contains(&WasmCapability::DomRead));
        assert!(caps.contains(&WasmCapability::DomWrite));
        assert!(!caps.contains(&WasmCapability::FileSystem));
        assert!(!caps.contains(&WasmCapability::AgentSpawn));
    }

    #[test]
    fn test_all_capabilities() {
        let all = WasmCapability::all();
        assert!(all.contains(&WasmCapability::FileSystem));
        assert!(all.contains(&WasmCapability::AgentSpawn));
        assert_eq!(all.len(), 16);
    }

    #[test]
    fn test_untrusted_policy() {
        let policy = SandboxPolicy::untrusted("test");
        assert!(policy.has_capability(WasmCapability::DomRead));
        assert!(!policy.has_capability(WasmCapability::FileSystem));
        assert!(policy.knowledge_isolation);
        assert_eq!(policy.limits.max_memory_bytes, 4 * 1024 * 1024);
    }

    #[test]
    fn test_trusted_policy() {
        let policy = SandboxPolicy::trusted("test");
        assert!(policy.has_capability(WasmCapability::FileSystem));
        assert!(!policy.knowledge_isolation);
    }

    #[test]
    fn test_url_filtering() {
        let mut policy = SandboxPolicy::untrusted("test");
        policy.allowed_url_patterns = vec!["https://api.example.com/*".to_string()];
        policy.blocked_url_patterns = vec!["*internal*".to_string()];

        assert!(policy.is_url_allowed("https://api.example.com/v1/data"));
        assert!(!policy.is_url_allowed("https://evil.com/hack"));
        assert!(!policy.is_url_allowed("https://api.example.com/internal/secret"));
    }

    #[test]
    fn test_sandbox_capability_check() {
        let agent_id = AgentId::new();
        let policy = SandboxPolicy::untrusted("test");
        let mut sandbox = SandboxInstance::new(agent_id, policy);

        assert!(sandbox.check_capability(WasmCapability::DomRead).is_ok());
        assert!(sandbox.check_capability(WasmCapability::FileSystem).is_err());
        assert!(sandbox.has_violations());
        assert_eq!(sandbox.violations.len(), 1);
    }

    #[test]
    fn test_sandbox_instruction_limits() {
        let agent_id = AgentId::new();
        let mut policy = SandboxPolicy::untrusted("test");
        policy.limits.max_instructions = 100;
        let mut sandbox = SandboxInstance::new(agent_id, policy);

        assert!(sandbox.consume_instructions(50).is_ok());
        assert!(sandbox.consume_instructions(60).is_err()); // 110 > 100
    }

    #[test]
    fn test_sandbox_element_limits() {
        let agent_id = AgentId::new();
        let mut policy = SandboxPolicy::untrusted("test");
        policy.limits.max_elements = 2;
        let mut sandbox = SandboxInstance::new(agent_id, policy);

        assert!(sandbox.track_element().is_ok());
        assert!(sandbox.track_element().is_ok());
        assert!(sandbox.track_element().is_err()); // 3 > 2
    }

    #[test]
    fn test_sandbox_registry() {
        let mut registry = SandboxRegistry::new();
        let agent_id = AgentId::new();

        registry.create_sandbox(agent_id, "untrusted").unwrap();
        assert_eq!(registry.active_count(), 1);

        let sandbox = registry.get_sandbox_mut(&agent_id).unwrap();
        assert!(sandbox.check_capability(WasmCapability::DomRead).is_ok());

        let removed = registry.remove_sandbox(&agent_id).unwrap();
        assert_eq!(removed.agent_id, agent_id);
        assert_eq!(registry.active_count(), 0);
    }

    #[test]
    fn test_sandbox_usage_summary() {
        let agent_id = AgentId::new();
        let policy = SandboxPolicy::untrusted("test");
        let mut sandbox = SandboxInstance::new(agent_id, policy);

        sandbox.consume_instructions(500).unwrap();
        sandbox.track_element().unwrap();
        sandbox.track_event().unwrap();

        let summary = sandbox.usage_summary();
        assert_eq!(summary.instructions_used, 500);
        assert_eq!(summary.elements_created, 1);
        assert_eq!(summary.events_emitted, 1);
        assert_eq!(summary.violations_count, 0);
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*.com", "example.com"));
        assert!(!glob_match("*.com", "example.org"));
        assert!(glob_match("https://*", "https://example.com"));
        assert!(glob_match("exact", "exact"));
        assert!(!glob_match("exact", "other"));
    }
}
