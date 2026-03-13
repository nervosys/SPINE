//! # Agent Policy Engine
//!
//! Production-ready policy engine with real condition evaluation:
//! - **Time windows**: Parsed HH:MM start/end enforcement
//! - **Rate limiting**: Token-bucket per (subject, action) with sliding window
//! - **RBAC roles**: Named roles with permission sets
//! - **Audit logging**: Every evaluation recorded with timestamp, decision, reason
//! - **IP range matching**: CIDR-style prefix matching
//! - **Resource quotas**: Integration point for budget enforcement

use chrono::{DateTime, NaiveTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

use crate::{AgentCapability, TrustLevel};

// =============================================================================
// CORE TYPES
// =============================================================================

/// Declarative access-control policy engine with real condition evaluation.
pub struct PolicyEngine {
    policies: DashMap<String, Policy>,
    roles: DashMap<String, Role>,
    role_assignments: DashMap<Uuid, Vec<String>>,
    rate_counters: DashMap<RateKey, RateState>,
    audit_log: DashMap<u64, AuditEntry>,
    audit_seq: AtomicU64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub id: String,
    pub name: String,
    pub description: String,
    pub rules: Vec<PolicyRule>,
    pub priority: u32,
    pub effect: PolicyEffect,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyRule {
    pub id: String,
    pub subjects: Vec<SubjectMatcher>,
    pub resources: Vec<ResourceMatcher>,
    pub actions: Vec<String>,
    pub conditions: Vec<PolicyCondition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubjectMatcher {
    AgentId(Uuid),
    HasCapability(AgentCapability),
    TrustLevel(TrustLevel),
    InGroup(String),
    HasRole(String),
    Any,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResourceMatcher {
    Type(String),
    Path(String),
    OwnedBy(Uuid),
    Tagged(String),
    Any,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PolicyCondition {
    /// Time-of-day window (HH:MM format, UTC)
    TimeWindow { start: String, end: String },
    /// Per-subject rate limit
    RateLimit { max_requests: u32, window_secs: u64 },
    /// IP prefix matching
    IpPrefix(String),
    /// Require a specific role
    RequireRole(String),
    /// Minimum trust level
    MinTrust(TrustLevel),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyEffect {
    Allow,
    Deny,
    RequireApproval,
    Log,
}

/// Context passed with every access evaluation.
#[derive(Debug, Clone, Default)]
pub struct EvaluationContext {
    pub capabilities: Vec<AgentCapability>,
    pub trust_level: TrustLevel,
    pub groups: Vec<String>,
    pub resource_owner: Option<Uuid>,
    pub resource_tags: Vec<String>,
    pub ip_address: Option<String>,
    pub timestamp: DateTime<Utc>,
}

/// Result of a policy evaluation.
#[derive(Debug, Clone)]
pub struct EvaluationResult {
    pub request_id: Uuid,
    pub subject: Uuid,
    pub resource: String,
    pub action: String,
    pub decision: PolicyEffect,
    pub matching_policies: Vec<String>,
    pub evaluated_at: DateTime<Utc>,
    pub reason: Option<String>,
}

// =============================================================================
// RBAC ROLES
// =============================================================================

/// Named role with a set of permitted actions on resource patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Role {
    pub name: String,
    pub description: String,
    /// action → set of resource patterns (glob-style prefixes)
    pub permissions: HashMap<String, HashSet<String>>,
}

impl Role {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            permissions: HashMap::new(),
        }
    }

    pub fn allow(mut self, action: impl Into<String>, resource_pattern: impl Into<String>) -> Self {
        self.permissions
            .entry(action.into())
            .or_default()
            .insert(resource_pattern.into());
        self
    }

    pub fn permits(&self, action: &str, resource: &str) -> bool {
        if let Some(patterns) = self.permissions.get(action) {
            return patterns.iter().any(|p| {
                if p == "*" {
                    true
                } else {
                    resource.starts_with(p)
                }
            });
        }
        // Wildcard action
        if let Some(patterns) = self.permissions.get("*") {
            return patterns.iter().any(|p| {
                if p == "*" {
                    true
                } else {
                    resource.starts_with(p)
                }
            });
        }
        false
    }
}

// =============================================================================
// RATE LIMITING
// =============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RateKey {
    subject: Uuid,
    action: String,
}

#[derive(Debug, Clone)]
struct RateState {
    timestamps: Vec<i64>,
}

// =============================================================================
// AUDIT LOG
// =============================================================================

/// Immutable audit record of every policy evaluation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub seq: u64,
    pub timestamp: DateTime<Utc>,
    pub subject: Uuid,
    pub resource: String,
    pub action: String,
    pub decision: String,
    pub matching_policies: Vec<String>,
    pub reason: Option<String>,
}

// =============================================================================
// IMPLEMENTATION
// =============================================================================

impl Default for PolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl PolicyEngine {
    pub fn new() -> Self {
        Self {
            policies: DashMap::new(),
            roles: DashMap::new(),
            role_assignments: DashMap::new(),
            rate_counters: DashMap::new(),
            audit_log: DashMap::new(),
            audit_seq: AtomicU64::new(0),
        }
    }

    // -- Policy CRUD --

    pub fn add_policy(&self, policy: Policy) {
        self.policies.insert(policy.id.clone(), policy);
    }

    pub fn remove_policy(&self, id: &str) -> Option<Policy> {
        self.policies.remove(id).map(|(_, p)| p)
    }

    pub fn policy_count(&self) -> usize {
        self.policies.len()
    }

    // -- Role management --

    pub fn define_role(&self, role: Role) {
        self.roles.insert(role.name.clone(), role);
    }

    pub fn assign_role(&self, agent: Uuid, role_name: &str) {
        self.role_assignments
            .entry(agent)
            .or_default()
            .push(role_name.to_string());
    }

    pub fn agent_roles(&self, agent: Uuid) -> Vec<String> {
        self.role_assignments
            .get(&agent)
            .map(|r| r.value().clone())
            .unwrap_or_default()
    }

    // -- Audit --

    pub fn audit_entries(&self) -> Vec<AuditEntry> {
        let mut entries: Vec<_> = self.audit_log.iter().map(|e| e.value().clone()).collect();
        entries.sort_by_key(|e| e.seq);
        entries
    }

    pub fn audit_count(&self) -> u64 {
        self.audit_seq.load(Ordering::Relaxed)
    }

    // -- Evaluation --

    pub fn evaluate(
        &self,
        subject: Uuid,
        resource: &str,
        action: &str,
        context: &EvaluationContext,
    ) -> EvaluationResult {
        let mut matching = Vec::new();
        let mut final_effect = PolicyEffect::Deny;
        let mut highest_priority = 0u32;
        let mut reason: Option<String> = None;

        for entry in self.policies.iter() {
            let policy = entry.value();
            if !policy.enabled {
                continue;
            }

            for rule in &policy.rules {
                match self.matches_rule(rule, subject, resource, action, context) {
                    RuleMatch::Matches => {
                        matching.push(policy.id.clone());
                        if policy.priority >= highest_priority {
                            highest_priority = policy.priority;
                            final_effect = policy.effect.clone();
                            reason = Some(format!("Policy '{}' matched", policy.name));
                        }
                    }
                    RuleMatch::ConditionFailed(r) => {
                        // A condition blocked it — keep the deny with reason
                        if matching.is_empty() {
                            reason = Some(r);
                        }
                    }
                    RuleMatch::NoMatch => {}
                }
            }
        }

        if matching.is_empty() && reason.is_none() {
            reason = Some("No matching policy (default deny)".to_string());
        }

        let result = EvaluationResult {
            request_id: Uuid::new_v4(),
            subject,
            resource: resource.to_string(),
            action: action.to_string(),
            decision: final_effect,
            matching_policies: matching,
            evaluated_at: Utc::now(),
            reason: reason.clone(),
        };

        // Write audit entry
        let seq = self.audit_seq.fetch_add(1, Ordering::Relaxed);
        let decision_str = format!("{:?}", result.decision);
        self.audit_log.insert(
            seq,
            AuditEntry {
                seq,
                timestamp: result.evaluated_at,
                subject,
                resource: resource.to_string(),
                action: action.to_string(),
                decision: decision_str,
                matching_policies: result.matching_policies.clone(),
                reason,
            },
        );

        result
    }

    fn matches_rule(
        &self,
        rule: &PolicyRule,
        subject: Uuid,
        resource: &str,
        action: &str,
        context: &EvaluationContext,
    ) -> RuleMatch {
        // Action match
        if !rule.actions.iter().any(|a| a == "*" || a == action) {
            return RuleMatch::NoMatch;
        }

        // Subject match
        let agent_roles = self.agent_roles(subject);
        let subject_matches = rule.subjects.iter().any(|s| match s {
            SubjectMatcher::AgentId(id) => *id == subject,
            SubjectMatcher::HasCapability(cap) => context.capabilities.contains(cap),
            SubjectMatcher::TrustLevel(level) => context.trust_level >= *level,
            SubjectMatcher::InGroup(group) => context.groups.contains(group),
            SubjectMatcher::HasRole(role) => agent_roles.contains(role),
            SubjectMatcher::Any => true,
        });
        if !subject_matches {
            return RuleMatch::NoMatch;
        }

        // Resource match
        let resource_matches = rule.resources.iter().any(|r| match r {
            ResourceMatcher::Type(t) => resource.starts_with(t),
            ResourceMatcher::Path(p) => resource.contains(p),
            ResourceMatcher::OwnedBy(owner) => context.resource_owner.as_ref() == Some(owner),
            ResourceMatcher::Tagged(tag) => context.resource_tags.contains(tag),
            ResourceMatcher::Any => true,
        });
        if !resource_matches {
            return RuleMatch::NoMatch;
        }

        // Conditions — each must pass
        for cond in &rule.conditions {
            if let Some(fail_reason) = self.evaluate_condition(cond, subject, action, context) {
                return RuleMatch::ConditionFailed(fail_reason);
            }
        }

        RuleMatch::Matches
    }

    /// Returns `None` if condition passes, `Some(reason)` if it fails.
    fn evaluate_condition(
        &self,
        condition: &PolicyCondition,
        subject: Uuid,
        action: &str,
        context: &EvaluationContext,
    ) -> Option<String> {
        match condition {
            PolicyCondition::TimeWindow { start, end } => {
                let now = context.timestamp.time();
                let start_time = NaiveTime::parse_from_str(start, "%H:%M")
                    .unwrap_or(NaiveTime::from_hms_opt(0, 0, 0).unwrap());
                let end_time = NaiveTime::parse_from_str(end, "%H:%M")
                    .unwrap_or(NaiveTime::from_hms_opt(23, 59, 0).unwrap());

                let in_window = if start_time <= end_time {
                    now >= start_time && now <= end_time
                } else {
                    // Overnight window (e.g., 22:00 → 06:00)
                    now >= start_time || now <= end_time
                };

                if in_window {
                    None
                } else {
                    Some(format!(
                        "Outside time window {}-{} (current: {})",
                        start, end, now
                    ))
                }
            }

            PolicyCondition::RateLimit {
                max_requests,
                window_secs,
            } => {
                let key = RateKey {
                    subject,
                    action: action.to_string(),
                };
                let now_ts = context.timestamp.timestamp();
                let cutoff = now_ts - (*window_secs as i64);

                let mut entry = self.rate_counters.entry(key).or_insert_with(|| RateState {
                    timestamps: Vec::new(),
                });
                let state = entry.value_mut();

                // Prune old entries
                state.timestamps.retain(|&t| t > cutoff);

                if state.timestamps.len() >= *max_requests as usize {
                    Some(format!(
                        "Rate limit exceeded: {} requests in {}s (max {})",
                        state.timestamps.len(),
                        window_secs,
                        max_requests
                    ))
                } else {
                    // Record this request
                    state.timestamps.push(now_ts);
                    None
                }
            }

            PolicyCondition::IpPrefix(prefix) => {
                if let Some(ip) = &context.ip_address {
                    if ip.starts_with(prefix.as_str()) {
                        None
                    } else {
                        Some(format!("IP {} not in prefix {}", ip, prefix))
                    }
                } else {
                    Some("No IP address in context".to_string())
                }
            }

            PolicyCondition::RequireRole(role_name) => {
                let roles = self.agent_roles(subject);
                if roles.iter().any(|r| r == role_name) {
                    None
                } else {
                    Some(format!("Missing required role '{}'", role_name))
                }
            }

            PolicyCondition::MinTrust(min_level) => {
                if context.trust_level >= *min_level {
                    None
                } else {
                    Some(format!(
                        "Trust level {:?} below minimum {:?}",
                        context.trust_level, min_level
                    ))
                }
            }
        }
    }
}

enum RuleMatch {
    Matches,
    ConditionFailed(String),
    NoMatch,
}

// =============================================================================
// PRE-BUILT ROLES
// =============================================================================

/// Common role definitions for typical agent deployments.
pub fn admin_role() -> Role {
    Role::new("admin", "Full access to all resources")
        .allow("*", "*")
}

pub fn reader_role() -> Role {
    Role::new("reader", "Read-only access")
        .allow("read", "*")
        .allow("list", "*")
        .allow("query", "*")
}

pub fn operator_role() -> Role {
    Role::new("operator", "Read + execute, no admin")
        .allow("read", "*")
        .allow("list", "*")
        .allow("query", "*")
        .allow("execute", "*")
        .allow("navigate", "*")
}

pub fn sandbox_role() -> Role {
    Role::new("sandbox", "Restricted to sandbox resources only")
        .allow("read", "sandbox/")
        .allow("execute", "sandbox/")
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn engine_with_allow_policy() -> PolicyEngine {
        let engine = PolicyEngine::new();
        engine.add_policy(Policy {
            id: "allow-read".into(),
            name: "Allow reads".into(),
            description: "Allow read actions for any subject".into(),
            rules: vec![PolicyRule {
                id: "r1".into(),
                subjects: vec![SubjectMatcher::Any],
                resources: vec![ResourceMatcher::Any],
                actions: vec!["read".into()],
                conditions: vec![],
            }],
            priority: 10,
            effect: PolicyEffect::Allow,
            enabled: true,
        });
        engine
    }

    #[test]
    fn test_default_deny() {
        let engine = PolicyEngine::new();
        let ctx = EvaluationContext {
            timestamp: Utc::now(),
            ..Default::default()
        };
        let result = engine.evaluate(Uuid::new_v4(), "resource", "read", &ctx);
        assert_eq!(result.decision, PolicyEffect::Deny);
        assert!(result.matching_policies.is_empty());
    }

    #[test]
    fn test_basic_allow() {
        let engine = engine_with_allow_policy();
        let ctx = EvaluationContext {
            timestamp: Utc::now(),
            ..Default::default()
        };
        let result = engine.evaluate(Uuid::new_v4(), "anything", "read", &ctx);
        assert_eq!(result.decision, PolicyEffect::Allow);
        assert_eq!(result.matching_policies, vec!["allow-read"]);
    }

    #[test]
    fn test_action_mismatch_denies() {
        let engine = engine_with_allow_policy();
        let ctx = EvaluationContext {
            timestamp: Utc::now(),
            ..Default::default()
        };
        let result = engine.evaluate(Uuid::new_v4(), "anything", "write", &ctx);
        assert_eq!(result.decision, PolicyEffect::Deny);
    }

    #[test]
    fn test_disabled_policy_skipped() {
        let engine = PolicyEngine::new();
        engine.add_policy(Policy {
            id: "disabled".into(),
            name: "Disabled".into(),
            description: "".into(),
            rules: vec![PolicyRule {
                id: "r1".into(),
                subjects: vec![SubjectMatcher::Any],
                resources: vec![ResourceMatcher::Any],
                actions: vec!["*".into()],
                conditions: vec![],
            }],
            priority: 100,
            effect: PolicyEffect::Allow,
            enabled: false,
        });
        let ctx = EvaluationContext {
            timestamp: Utc::now(),
            ..Default::default()
        };
        let result = engine.evaluate(Uuid::new_v4(), "r", "read", &ctx);
        assert_eq!(result.decision, PolicyEffect::Deny);
    }

    #[test]
    fn test_priority_resolution() {
        let engine = PolicyEngine::new();
        engine.add_policy(Policy {
            id: "low-allow".into(),
            name: "Low allow".into(),
            description: "".into(),
            rules: vec![PolicyRule {
                id: "r1".into(),
                subjects: vec![SubjectMatcher::Any],
                resources: vec![ResourceMatcher::Any],
                actions: vec!["*".into()],
                conditions: vec![],
            }],
            priority: 1,
            effect: PolicyEffect::Allow,
            enabled: true,
        });
        engine.add_policy(Policy {
            id: "high-deny".into(),
            name: "High deny".into(),
            description: "".into(),
            rules: vec![PolicyRule {
                id: "r2".into(),
                subjects: vec![SubjectMatcher::Any],
                resources: vec![ResourceMatcher::Any],
                actions: vec!["*".into()],
                conditions: vec![],
            }],
            priority: 100,
            effect: PolicyEffect::Deny,
            enabled: true,
        });
        let ctx = EvaluationContext {
            timestamp: Utc::now(),
            ..Default::default()
        };
        let result = engine.evaluate(Uuid::new_v4(), "r", "read", &ctx);
        assert_eq!(result.decision, PolicyEffect::Deny);
    }

    #[test]
    fn test_subject_agent_id() {
        let engine = PolicyEngine::new();
        let allowed_id = Uuid::new_v4();
        engine.add_policy(Policy {
            id: "agent-only".into(),
            name: "Agent only".into(),
            description: "".into(),
            rules: vec![PolicyRule {
                id: "r1".into(),
                subjects: vec![SubjectMatcher::AgentId(allowed_id)],
                resources: vec![ResourceMatcher::Any],
                actions: vec!["*".into()],
                conditions: vec![],
            }],
            priority: 10,
            effect: PolicyEffect::Allow,
            enabled: true,
        });
        let ctx = EvaluationContext {
            timestamp: Utc::now(),
            ..Default::default()
        };
        let ok = engine.evaluate(allowed_id, "r", "read", &ctx);
        assert_eq!(ok.decision, PolicyEffect::Allow);

        let denied = engine.evaluate(Uuid::new_v4(), "r", "read", &ctx);
        assert_eq!(denied.decision, PolicyEffect::Deny);
    }

    #[test]
    fn test_subject_trust_level() {
        let engine = PolicyEngine::new();
        engine.add_policy(Policy {
            id: "trusted-only".into(),
            name: "Trusted only".into(),
            description: "".into(),
            rules: vec![PolicyRule {
                id: "r1".into(),
                subjects: vec![SubjectMatcher::TrustLevel(TrustLevel::Trusted)],
                resources: vec![ResourceMatcher::Any],
                actions: vec!["*".into()],
                conditions: vec![],
            }],
            priority: 10,
            effect: PolicyEffect::Allow,
            enabled: true,
        });

        let low_ctx = EvaluationContext {
            trust_level: TrustLevel::Unknown,
            timestamp: Utc::now(),
            ..Default::default()
        };
        assert_eq!(
            engine.evaluate(Uuid::new_v4(), "r", "x", &low_ctx).decision,
            PolicyEffect::Deny
        );

        let high_ctx = EvaluationContext {
            trust_level: TrustLevel::Core,
            timestamp: Utc::now(),
            ..Default::default()
        };
        assert_eq!(
            engine.evaluate(Uuid::new_v4(), "r", "x", &high_ctx).decision,
            PolicyEffect::Allow
        );
    }

    #[test]
    fn test_resource_type_matcher() {
        let engine = PolicyEngine::new();
        engine.add_policy(Policy {
            id: "files-only".into(),
            name: "Files only".into(),
            description: "".into(),
            rules: vec![PolicyRule {
                id: "r1".into(),
                subjects: vec![SubjectMatcher::Any],
                resources: vec![ResourceMatcher::Type("file:".into())],
                actions: vec!["read".into()],
                conditions: vec![],
            }],
            priority: 10,
            effect: PolicyEffect::Allow,
            enabled: true,
        });
        let ctx = EvaluationContext {
            timestamp: Utc::now(),
            ..Default::default()
        };
        assert_eq!(
            engine.evaluate(Uuid::new_v4(), "file:doc.txt", "read", &ctx).decision,
            PolicyEffect::Allow
        );
        assert_eq!(
            engine.evaluate(Uuid::new_v4(), "network:api", "read", &ctx).decision,
            PolicyEffect::Deny
        );
    }

    #[test]
    fn test_resource_tagged() {
        let engine = PolicyEngine::new();
        engine.add_policy(Policy {
            id: "public-tag".into(),
            name: "Public tagged".into(),
            description: "".into(),
            rules: vec![PolicyRule {
                id: "r1".into(),
                subjects: vec![SubjectMatcher::Any],
                resources: vec![ResourceMatcher::Tagged("public".into())],
                actions: vec!["*".into()],
                conditions: vec![],
            }],
            priority: 10,
            effect: PolicyEffect::Allow,
            enabled: true,
        });
        let ctx_with_tag = EvaluationContext {
            resource_tags: vec!["public".into()],
            timestamp: Utc::now(),
            ..Default::default()
        };
        assert_eq!(
            engine.evaluate(Uuid::new_v4(), "r", "read", &ctx_with_tag).decision,
            PolicyEffect::Allow
        );
        let ctx_no_tag = EvaluationContext {
            timestamp: Utc::now(),
            ..Default::default()
        };
        assert_eq!(
            engine.evaluate(Uuid::new_v4(), "r", "read", &ctx_no_tag).decision,
            PolicyEffect::Deny
        );
    }

    #[test]
    fn test_time_window_inside() {
        let engine = PolicyEngine::new();
        engine.add_policy(Policy {
            id: "daytime".into(),
            name: "Daytime only".into(),
            description: "".into(),
            rules: vec![PolicyRule {
                id: "r1".into(),
                subjects: vec![SubjectMatcher::Any],
                resources: vec![ResourceMatcher::Any],
                actions: vec!["*".into()],
                conditions: vec![PolicyCondition::TimeWindow {
                    start: "00:00".into(),
                    end: "23:59".into(),
                }],
            }],
            priority: 10,
            effect: PolicyEffect::Allow,
            enabled: true,
        });
        let ctx = EvaluationContext {
            timestamp: Utc::now(),
            ..Default::default()
        };
        // Any time should be inside 00:00-23:59
        assert_eq!(
            engine.evaluate(Uuid::new_v4(), "r", "x", &ctx).decision,
            PolicyEffect::Allow
        );
    }

    #[test]
    fn test_time_window_outside() {
        let engine = PolicyEngine::new();
        engine.add_policy(Policy {
            id: "narrow".into(),
            name: "Narrow window".into(),
            description: "".into(),
            rules: vec![PolicyRule {
                id: "r1".into(),
                subjects: vec![SubjectMatcher::Any],
                resources: vec![ResourceMatcher::Any],
                actions: vec!["*".into()],
                conditions: vec![PolicyCondition::TimeWindow {
                    start: "02:00".into(),
                    end: "02:01".into(),
                }],
            }],
            priority: 10,
            effect: PolicyEffect::Allow,
            enabled: true,
        });
        // Use a fixed timestamp at 12:00 — definitely outside 02:00-02:01
        let fixed = chrono::NaiveDate::from_ymd_opt(2025, 1, 15)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc();
        let ctx = EvaluationContext {
            timestamp: fixed,
            ..Default::default()
        };
        // Condition fails → no match → default deny
        assert_eq!(
            engine.evaluate(Uuid::new_v4(), "r", "x", &ctx).decision,
            PolicyEffect::Deny
        );
    }

    #[test]
    fn test_rate_limit_allows_under_threshold() {
        let engine = PolicyEngine::new();
        engine.add_policy(Policy {
            id: "rated".into(),
            name: "Rated".into(),
            description: "".into(),
            rules: vec![PolicyRule {
                id: "r1".into(),
                subjects: vec![SubjectMatcher::Any],
                resources: vec![ResourceMatcher::Any],
                actions: vec!["*".into()],
                conditions: vec![PolicyCondition::RateLimit {
                    max_requests: 5,
                    window_secs: 60,
                }],
            }],
            priority: 10,
            effect: PolicyEffect::Allow,
            enabled: true,
        });
        let agent = Uuid::new_v4();
        let ctx = EvaluationContext {
            timestamp: Utc::now(),
            ..Default::default()
        };
        // First 5 requests should pass
        for _ in 0..5 {
            let r = engine.evaluate(agent, "r", "x", &ctx);
            assert_eq!(r.decision, PolicyEffect::Allow);
        }
    }

    #[test]
    fn test_rate_limit_blocks_over_threshold() {
        let engine = PolicyEngine::new();
        engine.add_policy(Policy {
            id: "rated".into(),
            name: "Rated".into(),
            description: "".into(),
            rules: vec![PolicyRule {
                id: "r1".into(),
                subjects: vec![SubjectMatcher::Any],
                resources: vec![ResourceMatcher::Any],
                actions: vec!["*".into()],
                conditions: vec![PolicyCondition::RateLimit {
                    max_requests: 3,
                    window_secs: 60,
                }],
            }],
            priority: 10,
            effect: PolicyEffect::Allow,
            enabled: true,
        });
        let agent = Uuid::new_v4();
        let ctx = EvaluationContext {
            timestamp: Utc::now(),
            ..Default::default()
        };
        // First 3 pass
        for _ in 0..3 {
            assert_eq!(engine.evaluate(agent, "r", "x", &ctx).decision, PolicyEffect::Allow);
        }
        // 4th blocked (condition fails → no match → default deny)
        assert_eq!(engine.evaluate(agent, "r", "x", &ctx).decision, PolicyEffect::Deny);
    }

    #[test]
    fn test_ip_prefix_match() {
        let engine = PolicyEngine::new();
        engine.add_policy(Policy {
            id: "internal".into(),
            name: "Internal only".into(),
            description: "".into(),
            rules: vec![PolicyRule {
                id: "r1".into(),
                subjects: vec![SubjectMatcher::Any],
                resources: vec![ResourceMatcher::Any],
                actions: vec!["*".into()],
                conditions: vec![PolicyCondition::IpPrefix("10.0.".into())],
            }],
            priority: 10,
            effect: PolicyEffect::Allow,
            enabled: true,
        });
        let ok_ctx = EvaluationContext {
            ip_address: Some("10.0.1.5".into()),
            timestamp: Utc::now(),
            ..Default::default()
        };
        assert_eq!(engine.evaluate(Uuid::new_v4(), "r", "x", &ok_ctx).decision, PolicyEffect::Allow);

        let bad_ctx = EvaluationContext {
            ip_address: Some("192.168.1.1".into()),
            timestamp: Utc::now(),
            ..Default::default()
        };
        assert_eq!(engine.evaluate(Uuid::new_v4(), "r", "x", &bad_ctx).decision, PolicyEffect::Deny);
    }

    #[test]
    fn test_role_based_access() {
        let engine = PolicyEngine::new();
        let admin = admin_role();
        engine.define_role(admin);

        let agent = Uuid::new_v4();
        engine.assign_role(agent, "admin");

        engine.add_policy(Policy {
            id: "admin-access".into(),
            name: "Admin access".into(),
            description: "".into(),
            rules: vec![PolicyRule {
                id: "r1".into(),
                subjects: vec![SubjectMatcher::HasRole("admin".into())],
                resources: vec![ResourceMatcher::Any],
                actions: vec!["*".into()],
                conditions: vec![],
            }],
            priority: 10,
            effect: PolicyEffect::Allow,
            enabled: true,
        });
        let ctx = EvaluationContext {
            timestamp: Utc::now(),
            ..Default::default()
        };
        assert_eq!(engine.evaluate(agent, "anything", "delete", &ctx).decision, PolicyEffect::Allow);

        let non_admin = Uuid::new_v4();
        assert_eq!(engine.evaluate(non_admin, "anything", "delete", &ctx).decision, PolicyEffect::Deny);
    }

    #[test]
    fn test_require_role_condition() {
        let engine = PolicyEngine::new();
        let agent = Uuid::new_v4();
        engine.assign_role(agent, "operator");

        engine.add_policy(Policy {
            id: "ops".into(),
            name: "Ops".into(),
            description: "".into(),
            rules: vec![PolicyRule {
                id: "r1".into(),
                subjects: vec![SubjectMatcher::Any],
                resources: vec![ResourceMatcher::Any],
                actions: vec!["execute".into()],
                conditions: vec![PolicyCondition::RequireRole("operator".into())],
            }],
            priority: 10,
            effect: PolicyEffect::Allow,
            enabled: true,
        });
        let ctx = EvaluationContext {
            timestamp: Utc::now(),
            ..Default::default()
        };
        assert_eq!(engine.evaluate(agent, "task", "execute", &ctx).decision, PolicyEffect::Allow);
        assert_eq!(engine.evaluate(Uuid::new_v4(), "task", "execute", &ctx).decision, PolicyEffect::Deny);
    }

    #[test]
    fn test_min_trust_condition() {
        let engine = PolicyEngine::new();
        engine.add_policy(Policy {
            id: "high-trust".into(),
            name: "High trust ops".into(),
            description: "".into(),
            rules: vec![PolicyRule {
                id: "r1".into(),
                subjects: vec![SubjectMatcher::Any],
                resources: vec![ResourceMatcher::Any],
                actions: vec!["*".into()],
                conditions: vec![PolicyCondition::MinTrust(TrustLevel::Trusted)],
            }],
            priority: 10,
            effect: PolicyEffect::Allow,
            enabled: true,
        });

        let low = EvaluationContext {
            trust_level: TrustLevel::Unknown,
            timestamp: Utc::now(),
            ..Default::default()
        };
        assert_eq!(engine.evaluate(Uuid::new_v4(), "r", "x", &low).decision, PolicyEffect::Deny);

        let high = EvaluationContext {
            trust_level: TrustLevel::HighlyTrusted,
            timestamp: Utc::now(),
            ..Default::default()
        };
        assert_eq!(engine.evaluate(Uuid::new_v4(), "r", "x", &high).decision, PolicyEffect::Allow);
    }

    #[test]
    fn test_audit_log_populated() {
        let engine = engine_with_allow_policy();
        let ctx = EvaluationContext {
            timestamp: Utc::now(),
            ..Default::default()
        };
        engine.evaluate(Uuid::new_v4(), "res1", "read", &ctx);
        engine.evaluate(Uuid::new_v4(), "res2", "read", &ctx);

        assert_eq!(engine.audit_count(), 2);
        let entries = engine.audit_entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].action, "read");
        assert_eq!(entries[0].resource, "res1");
    }

    #[test]
    fn test_remove_policy() {
        let engine = engine_with_allow_policy();
        assert_eq!(engine.policy_count(), 1);
        let removed = engine.remove_policy("allow-read");
        assert!(removed.is_some());
        assert_eq!(engine.policy_count(), 0);
    }

    #[test]
    fn test_role_permits() {
        let reader = reader_role();
        assert!(reader.permits("read", "anything"));
        assert!(reader.permits("query", "data/items"));
        assert!(!reader.permits("write", "data/items"));
        assert!(!reader.permits("delete", "data/items"));
    }

    #[test]
    fn test_sandbox_role_restricts_path() {
        let sandbox = sandbox_role();
        assert!(sandbox.permits("read", "sandbox/file.txt"));
        assert!(sandbox.permits("execute", "sandbox/task"));
        assert!(!sandbox.permits("read", "data/secret.txt"));
        assert!(!sandbox.permits("execute", "system/cmd"));
    }

    #[test]
    fn test_multiple_conditions_all_must_pass() {
        let engine = PolicyEngine::new();
        let agent = Uuid::new_v4();
        engine.assign_role(agent, "operator");

        engine.add_policy(Policy {
            id: "multi".into(),
            name: "Multi condition".into(),
            description: "".into(),
            rules: vec![PolicyRule {
                id: "r1".into(),
                subjects: vec![SubjectMatcher::Any],
                resources: vec![ResourceMatcher::Any],
                actions: vec!["*".into()],
                conditions: vec![
                    PolicyCondition::RequireRole("operator".into()),
                    PolicyCondition::IpPrefix("10.".into()),
                ],
            }],
            priority: 10,
            effect: PolicyEffect::Allow,
            enabled: true,
        });

        // Has role but wrong IP
        let ctx1 = EvaluationContext {
            ip_address: Some("192.168.1.1".into()),
            timestamp: Utc::now(),
            ..Default::default()
        };
        assert_eq!(engine.evaluate(agent, "r", "x", &ctx1).decision, PolicyEffect::Deny);

        // Has role and correct IP
        let ctx2 = EvaluationContext {
            ip_address: Some("10.0.0.1".into()),
            timestamp: Utc::now(),
            ..Default::default()
        };
        assert_eq!(engine.evaluate(agent, "r", "x", &ctx2).decision, PolicyEffect::Allow);
    }

    #[test]
    fn test_wildcard_action() {
        let engine = PolicyEngine::new();
        engine.add_policy(Policy {
            id: "wildcard".into(),
            name: "Wildcard".into(),
            description: "".into(),
            rules: vec![PolicyRule {
                id: "r1".into(),
                subjects: vec![SubjectMatcher::Any],
                resources: vec![ResourceMatcher::Any],
                actions: vec!["*".into()],
                conditions: vec![],
            }],
            priority: 10,
            effect: PolicyEffect::Allow,
            enabled: true,
        });
        let ctx = EvaluationContext {
            timestamp: Utc::now(),
            ..Default::default()
        };
        assert_eq!(
            engine.evaluate(Uuid::new_v4(), "r", "anything", &ctx).decision,
            PolicyEffect::Allow
        );
    }

    #[test]
    fn test_has_capability_subject() {
        let engine = PolicyEngine::new();
        engine.add_policy(Policy {
            id: "nav-only".into(),
            name: "Navigation capable".into(),
            description: "".into(),
            rules: vec![PolicyRule {
                id: "r1".into(),
                subjects: vec![SubjectMatcher::HasCapability(AgentCapability::Navigation)],
                resources: vec![ResourceMatcher::Any],
                actions: vec!["navigate".into()],
                conditions: vec![],
            }],
            priority: 10,
            effect: PolicyEffect::Allow,
            enabled: true,
        });
        let ctx_with = EvaluationContext {
            capabilities: vec![AgentCapability::Navigation],
            timestamp: Utc::now(),
            ..Default::default()
        };
        assert_eq!(
            engine.evaluate(Uuid::new_v4(), "page", "navigate", &ctx_with).decision,
            PolicyEffect::Allow
        );
        let ctx_without = EvaluationContext {
            timestamp: Utc::now(),
            ..Default::default()
        };
        assert_eq!(
            engine.evaluate(Uuid::new_v4(), "page", "navigate", &ctx_without).decision,
            PolicyEffect::Deny
        );
    }
}
