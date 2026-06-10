//! Collaborative-SWE primitives over the SPINE agentic substrate.
//!
//! Two building blocks that make software-engineering collaboration first-class
//! on top of the existing messaging / swarm / consensus / knowledge layers:
//!
//! - [`SweArtifact`] — a **content-addressed, versioned** build/code artifact
//!   with provenance (`producer`), lineage (`supersedes`), and an optional
//!   Ed25519 signature over its content hash. Pairs with the no-exec ABL
//!   payloads MechGen agents exchange.
//! - [`WorkGraph`] — a **dependency-aware task DAG** for decomposed SWE work:
//!   tasks declare deps + required capabilities, become `Ready` when their deps
//!   complete, and are claimed → completed (recording the produced artifact).
//!
//! These are pure data + logic (no new wire types); they ride the existing
//! `KnowledgeShare` / `ActionRequest` messages and `SwarmCoordinator`.

use crate::{AgentCapability, AgentId};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use uuid::Uuid;

// ── Versioned, content-addressed artifacts ───────────────────────────────────

/// What a [`SweArtifact`] holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SweArtifactKind {
    /// An ABL neural-net container.
    AblNet,
    /// An ABL knowledge-base container.
    AblKb,
    /// An ABL agent/swarm container.
    AblAgentic,
    /// Source text.
    Source,
    /// A build or run log.
    BuildLog,
    /// A test / review report.
    TestReport,
    /// A diff / patch.
    Diff,
    /// Anything else (named).
    Other(String),
}

/// A content-addressed, versioned SWE artifact with provenance and lineage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SweArtifact {
    /// SHA-256 of the bytes, lowercase hex — the content address / cache key.
    pub content_hash: String,
    /// What this artifact is.
    pub kind: SweArtifactKind,
    /// Which agent produced it.
    pub producer: AgentId,
    /// The artifact this one replaces (its content hash), if any — version lineage.
    pub supersedes: Option<String>,
    /// Byte length of the content.
    pub size: usize,
    /// When it was produced.
    pub created_at: DateTime<Utc>,
    /// Ed25519 signature over `content_hash` bytes (set by [`Self::sign`]).
    pub signature: Option<Vec<u8>>,
}

impl SweArtifact {
    /// Content-address `bytes` and record provenance. Deterministic hash.
    pub fn new(bytes: &[u8], kind: SweArtifactKind, producer: AgentId) -> Self {
        let mut h = Sha256::new();
        h.update(bytes);
        SweArtifact {
            content_hash: format!("{:x}", h.finalize()),
            kind,
            producer,
            supersedes: None,
            size: bytes.len(),
            created_at: Utc::now(),
            signature: None,
        }
    }

    /// Mark this artifact as superseding a prior one (by content hash).
    pub fn superseding(mut self, prior: impl Into<String>) -> Self {
        self.supersedes = Some(prior.into());
        self
    }

    /// Sign the content hash with an Ed25519 key (integrity + authenticity).
    pub fn sign(&mut self, key: &ed25519_dalek::SigningKey) {
        use ed25519_dalek::Signer;
        self.signature = Some(key.sign(self.content_hash.as_bytes()).to_bytes().to_vec());
    }

    /// Verify the signature against a public key. `false` if unsigned or invalid.
    pub fn verify(&self, vk: &ed25519_dalek::VerifyingKey) -> bool {
        use ed25519_dalek::Verifier;
        let Some(sig_bytes) = &self.signature else { return false };
        let Ok(sig) = ed25519_dalek::Signature::from_slice(sig_bytes) else { return false };
        vk.verify(self.content_hash.as_bytes(), &sig).is_ok()
    }
}

/// A registry of artifacts keyed by content hash, with version lineage queries.
#[derive(Debug, Clone, Default)]
pub struct SweArtifactStore {
    by_hash: HashMap<String, SweArtifact>,
}

impl SweArtifactStore {
    /// Empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert (content-addressed, so re-inserting identical content is a no-op).
    pub fn insert(&mut self, artifact: SweArtifact) {
        self.by_hash.insert(artifact.content_hash.clone(), artifact);
    }

    /// Look up by content hash.
    pub fn get(&self, hash: &str) -> Option<&SweArtifact> {
        self.by_hash.get(hash)
    }

    /// The lineage of `hash`: itself, then each artifact it supersedes, in order.
    pub fn lineage(&self, hash: &str) -> Vec<&SweArtifact> {
        let mut out = Vec::new();
        let mut cur = self.by_hash.get(hash);
        // bounded by store size — supersedes chains can't exceed the # of artifacts
        let mut budget = self.by_hash.len() + 1;
        while let Some(a) = cur {
            out.push(a);
            if budget == 0 {
                break;
            }
            budget -= 1;
            cur = a.supersedes.as_deref().and_then(|h| self.by_hash.get(h));
        }
        out
    }

    /// Artifacts that nothing else supersedes — the current heads (deterministic order).
    pub fn heads(&self) -> Vec<&SweArtifact> {
        let superseded: std::collections::HashSet<&str> = self
            .by_hash
            .values()
            .filter_map(|a| a.supersedes.as_deref())
            .collect();
        let mut v: Vec<&SweArtifact> = self
            .by_hash
            .values()
            .filter(|a| !superseded.contains(a.content_hash.as_str()))
            .collect();
        v.sort_by(|a, b| a.content_hash.cmp(&b.content_hash));
        v
    }
}

// ── Dependency-aware work graph ──────────────────────────────────────────────

/// Status of a [`WorkTask`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// Has unmet dependencies.
    Blocked,
    /// All dependencies done; available to claim.
    Ready,
    /// Claimed by an agent, not yet finished.
    Claimed,
    /// Completed (produced an artifact).
    Done,
    /// Failed.
    Failed,
}

/// One unit of SWE work in a [`WorkGraph`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkTask {
    /// Stable id.
    pub id: Uuid,
    /// Human/agent-readable name.
    pub name: String,
    /// Tasks that must be `Done` before this one is `Ready`.
    pub deps: Vec<Uuid>,
    /// Capabilities an agent needs to claim it.
    pub required_capabilities: Vec<AgentCapability>,
    /// Current status.
    pub status: TaskStatus,
    /// Claiming agent, if any.
    pub assignee: Option<AgentId>,
    /// Content hash of the artifact this task produced (set on completion).
    pub artifact: Option<String>,
}

/// A dependency-aware DAG of SWE tasks with claim/complete semantics.
#[derive(Debug, Clone, Default)]
pub struct WorkGraph {
    tasks: HashMap<Uuid, WorkTask>,
}

impl WorkGraph {
    /// Empty graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a task; returns its id. A task with no (pending) deps starts `Ready`.
    pub fn add_task(
        &mut self,
        name: impl Into<String>,
        deps: Vec<Uuid>,
        required_capabilities: Vec<AgentCapability>,
    ) -> Uuid {
        let id = Uuid::new_v4();
        let status = if self.deps_satisfied(&deps) { TaskStatus::Ready } else { TaskStatus::Blocked };
        self.tasks.insert(
            id,
            WorkTask { id, name: name.into(), deps, required_capabilities, status, assignee: None, artifact: None },
        );
        id
    }

    fn deps_satisfied(&self, deps: &[Uuid]) -> bool {
        deps.iter().all(|d| self.tasks.get(d).map(|t| t.status == TaskStatus::Done).unwrap_or(false))
    }

    /// Tasks currently ready to claim (deterministic order by id).
    pub fn ready(&self) -> Vec<Uuid> {
        let mut v: Vec<Uuid> = self
            .tasks
            .values()
            .filter(|t| t.status == TaskStatus::Ready)
            .map(|t| t.id)
            .collect();
        v.sort();
        v
    }

    /// Claim a ready task for an agent. Errors if missing or not `Ready`.
    pub fn claim(&mut self, task: Uuid, agent: AgentId) -> Result<(), String> {
        let t = self.tasks.get_mut(&task).ok_or("unknown task")?;
        if t.status != TaskStatus::Ready {
            return Err(format!("task is {:?}, not Ready", t.status));
        }
        t.status = TaskStatus::Claimed;
        t.assignee = Some(agent);
        Ok(())
    }

    /// Complete a claimed task with the produced artifact's content hash; any
    /// dependents whose deps are now all `Done` transition `Blocked → Ready`.
    pub fn complete(&mut self, task: Uuid, artifact_hash: impl Into<String>) -> Result<(), String> {
        {
            let t = self.tasks.get_mut(&task).ok_or("unknown task")?;
            if t.status != TaskStatus::Claimed {
                return Err(format!("task is {:?}, not Claimed", t.status));
            }
            t.status = TaskStatus::Done;
            t.artifact = Some(artifact_hash.into());
        }
        // Recompute readiness of blocked tasks.
        let blocked: Vec<Uuid> = self
            .tasks
            .values()
            .filter(|t| t.status == TaskStatus::Blocked)
            .map(|t| t.id)
            .collect();
        for id in blocked {
            let deps = self.tasks[&id].deps.clone();
            if self.deps_satisfied(&deps) {
                self.tasks.get_mut(&id).unwrap().status = TaskStatus::Ready;
            }
        }
        Ok(())
    }

    /// All tasks done.
    pub fn is_complete(&self) -> bool {
        !self.tasks.is_empty() && self.tasks.values().all(|t| t.status == TaskStatus::Done)
    }

    /// A topological order of task ids, or `None` if the dependency graph has a
    /// cycle (Kahn's algorithm). Useful for validating a decomposition.
    pub fn topological_order(&self) -> Option<Vec<Uuid>> {
        let mut indegree: HashMap<Uuid, usize> =
            self.tasks.keys().map(|&id| (id, 0usize)).collect();
        for t in self.tasks.values() {
            for d in &t.deps {
                if self.tasks.contains_key(d) {
                    *indegree.get_mut(&t.id).unwrap() += 1;
                }
            }
        }
        let mut queue: Vec<Uuid> = indegree.iter().filter(|(_, &d)| d == 0).map(|(&id, _)| id).collect();
        queue.sort();
        let mut order = Vec::new();
        while let Some(id) = queue.pop() {
            order.push(id);
            // decrement dependents
            let mut newly: Vec<Uuid> = Vec::new();
            for t in self.tasks.values() {
                if t.deps.contains(&id) {
                    let e = indegree.get_mut(&t.id).unwrap();
                    *e -= 1;
                    if *e == 0 {
                        newly.push(t.id);
                    }
                }
            }
            newly.sort();
            queue.extend(newly);
        }
        if order.len() == self.tasks.len() {
            Some(order)
        } else {
            None // cycle
        }
    }

    /// Read-only view of a task.
    pub fn task(&self, id: Uuid) -> Option<&WorkTask> {
        self.tasks.get(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent() -> AgentId {
        AgentId::new()
    }

    #[test]
    fn artifact_is_content_addressed_and_deterministic() {
        let a = agent();
        let x = SweArtifact::new(b"ABL1\x02\x00net", SweArtifactKind::AblNet, a);
        let y = SweArtifact::new(b"ABL1\x02\x00net", SweArtifactKind::AblNet, a);
        assert_eq!(x.content_hash, y.content_hash, "same bytes → same hash");
        assert_eq!(x.size, 9); // "ABL1" + 0x02 + 0x00 + "net"
        let z = SweArtifact::new(b"different", SweArtifactKind::Source, a);
        assert_ne!(x.content_hash, z.content_hash);
    }

    #[test]
    fn artifact_sign_and_verify() {
        let key = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        let mut art = SweArtifact::new(b"build-1", SweArtifactKind::BuildLog, agent());
        assert!(!art.verify(&key.verifying_key()), "unsigned → not verified");
        art.sign(&key);
        assert!(art.verify(&key.verifying_key()), "signed → verifies");
        let other = ed25519_dalek::SigningKey::from_bytes(&[9u8; 32]);
        assert!(!art.verify(&other.verifying_key()), "wrong key → rejected");
    }

    #[test]
    fn store_lineage_and_heads() {
        let a = agent();
        let mut store = SweArtifactStore::new();
        let v1 = SweArtifact::new(b"v1", SweArtifactKind::Source, a);
        let v1h = v1.content_hash.clone();
        let v2 = SweArtifact::new(b"v2", SweArtifactKind::Source, a).superseding(v1h.clone());
        let v2h = v2.content_hash.clone();
        store.insert(v1);
        store.insert(v2);
        let lineage = store.lineage(&v2h);
        assert_eq!(lineage.len(), 2, "v2 → v1");
        assert_eq!(lineage[0].content_hash, v2h);
        assert_eq!(lineage[1].content_hash, v1h);
        let heads = store.heads();
        assert_eq!(heads.len(), 1, "only v2 is a head");
        assert_eq!(heads[0].content_hash, v2h);
    }

    #[test]
    fn work_graph_respects_dependencies() {
        let mut g = WorkGraph::new();
        let build = g.add_task("build", vec![], vec![AgentCapability::CodeExecution]);
        let test = g.add_task("test", vec![build], vec![AgentCapability::CodeExecution]);
        // Only `build` is ready; `test` is blocked on it.
        assert_eq!(g.ready(), vec![build]);
        assert_eq!(g.task(test).unwrap().status, TaskStatus::Blocked);
        // Claim + complete build → test becomes ready.
        let dev = agent();
        g.claim(build, dev).unwrap();
        g.complete(build, "hash-of-build").unwrap();
        assert_eq!(g.ready(), vec![test]);
        g.claim(test, dev).unwrap();
        g.complete(test, "hash-of-test").unwrap();
        assert!(g.is_complete());
    }

    #[test]
    fn work_graph_detects_topo_order_and_no_cycles() {
        let mut g = WorkGraph::new();
        let a = g.add_task("a", vec![], vec![]);
        let b = g.add_task("b", vec![a], vec![]);
        let _c = g.add_task("c", vec![b], vec![]);
        let order = g.topological_order().expect("acyclic");
        assert_eq!(order.len(), 3);
        // a must come before b before c
        let pos = |id| order.iter().position(|&x| x == id).unwrap();
        assert!(pos(a) < pos(b));
    }

    #[test]
    fn cannot_claim_blocked_or_complete_unclaimed() {
        let mut g = WorkGraph::new();
        let a = g.add_task("a", vec![], vec![]);
        let b = g.add_task("b", vec![a], vec![]);
        assert!(g.claim(b, agent()).is_err(), "b is blocked");
        assert!(g.complete(a, "h").is_err(), "a not claimed yet");
    }
}
