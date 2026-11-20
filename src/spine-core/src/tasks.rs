//! Durable read-only task worker with signing-key ownership and explicit admission.
use anyhow::{ensure, Context, Result};
use spine_protocol::{
    tasks::{TaskOperation, TaskRecord, TaskSpec, TaskState},
    ToolCall, ToolOutcome,
};
use spine_storage::StorageBackend;
use std::{
    collections::{BTreeSet, HashSet},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const TASKS: &str = "spine-tasks-v1";
const KNOWLEDGE: &str = "spine-task-knowledge-v1";
const MAX_TASKS: usize = 256;
const MAX_ARGS: usize = 64 * 1024;
const MAX_RESULT: usize = 256 * 1024;

pub struct TaskService {
    backend: Arc<dyn StorageBackend>,
    allowed: HashSet<[u8; 32]>,
    writes: Mutex<()>,
    pending: Mutex<BTreeSet<Vec<u8>>>,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn task_key(owner: &[u8; 32], id: Uuid) -> Vec<u8> {
    [owner.as_slice(), id.as_bytes().as_slice()].concat()
}
fn knowledge_key(owner: &[u8; 32], key: &str) -> Result<Vec<u8>> {
    ensure!(
        !key.is_empty() && key.len() <= 256,
        "knowledge key must contain 1 through 256 bytes"
    );
    Ok([owner.as_slice(), key.as_bytes()].concat())
}

impl TaskService {
    pub fn new(backend: Arc<dyn StorageBackend>, allowed: Vec<[u8; 32]>) -> Result<Self> {
        ensure!(
            !allowed.is_empty(),
            "task worker requires at least one allowed peer signing key"
        );
        let service = Self {
            backend,
            allowed: allowed.into_iter().collect(),
            writes: Mutex::new(()),
            pending: Mutex::new(BTreeSet::new()),
        };
        let records = service.backend.scan(TASKS, None)?;
        ensure!(
            records.len() <= MAX_TASKS,
            "task store exceeds configured capacity"
        );
        for (key, bytes) in records {
            let mut record: TaskRecord =
                serde_json::from_slice(&bytes).context("invalid task snapshot")?;
            ensure!(
                key == task_key(&record.owner, record.spec.id),
                "task snapshot key mismatch"
            );
            service.validate(&record.spec)?;
            if record.state == TaskState::Running {
                // Every admitted tool is read-only and deterministic; replay is
                // safe. This contract must change before adding side effects.
                record.state = TaskState::Pending;
                service.save(&record)?;
            }
            if record.state == TaskState::Pending && service.allowed.contains(&record.owner) {
                service.pending.lock().unwrap().insert(key);
            }
        }
        Ok(service)
    }

    fn validate(&self, spec: &TaskSpec) -> Result<()> {
        ensure!(
            matches!(
                spec.tool.as_str(),
                "spine.html/parse" | "spine.content/hash"
            ),
            "unsupported task tool"
        );
        ensure!(
            (1..=300).contains(&spec.timeout_secs),
            "task deadline must be 1 through 300 seconds"
        );
        ensure!(
            spec.dependencies.len() <= 16,
            "task supports at most 16 dependencies"
        );
        ensure!(
            !spec.dependencies.contains(&spec.id),
            "task cannot depend on itself"
        );
        ensure!(
            spec.args.is_object() && serde_json::to_vec(&spec.args)?.len() <= MAX_ARGS,
            "task args must be an object within 64 KiB"
        );
        if let Some(input) = &spec.input_from {
            ensure!(
                spec.dependencies.contains(&input.task_id),
                "input source must be a declared dependency"
            );
            ensure!(
                input.pointer.len() <= 256,
                "input pointer exceeds 256 bytes"
            );
        }
        Ok(())
    }
    fn get(&self, owner: &[u8; 32], id: Uuid) -> Result<TaskRecord> {
        let bytes = self
            .backend
            .get(TASKS, &task_key(owner, id))?
            .context("task not found for this owner")?;
        Ok(serde_json::from_slice(&bytes)?)
    }
    fn save(&self, record: &TaskRecord) -> Result<()> {
        self.backend.put(
            TASKS,
            &task_key(&record.owner, record.spec.id),
            &serde_json::to_vec(record)?,
        )?;
        let key = task_key(&record.owner, record.spec.id);
        if record.state == TaskState::Pending && self.allowed.contains(&record.owner) {
            self.pending.lock().unwrap().insert(key);
        } else {
            self.pending.lock().unwrap().remove(&key);
        }
        Ok(())
    }

    pub fn request(&self, caller: [u8; 32], operation: TaskOperation) -> Result<serde_json::Value> {
        ensure!(
            self.allowed.contains(&caller),
            "peer signing key is not authorized for tasks"
        );
        let _lock = self.writes.lock().unwrap();
        match operation {
            TaskOperation::Capabilities => {
                Ok(serde_json::json!({"protocol":"spine.tasks","version":1,
                "tools":crate::tools::capabilities(),"ownership":"caller signing key",
                "permissions":["task.submit","task.read","task.cancel","knowledge.read","knowledge.publish"],
                "limits":{"tasks":MAX_TASKS,"args_bytes":MAX_ARGS,"result_bytes":MAX_RESULT,"deadline_secs":300}}))
            }
            TaskOperation::Submit { spec } => {
                self.validate(&spec)?;
                if self.backend.exists(TASKS, &task_key(&caller, spec.id))? {
                    let existing = self.get(&caller, spec.id)?;
                    ensure!(
                        existing.spec == spec,
                        "task id conflicts with an existing specification"
                    );
                    return Ok(serde_json::to_value(existing)?);
                }
                ensure!(
                    self.backend.count(TASKS)? < MAX_TASKS,
                    "task store capacity reached"
                );
                // Dependencies must already exist in the owner's scope. Together
                // with self-reference refusal, this makes admission acyclic.
                for id in &spec.dependencies {
                    self.get(&caller, *id)?;
                }
                let record = TaskRecord {
                    owner: caller,
                    spec,
                    state: TaskState::Pending,
                    created_at: now(),
                    result: None,
                    error: None,
                };
                self.save(&record)?;
                Ok(serde_json::to_value(record)?)
            }
            TaskOperation::Status { id } => Ok(serde_json::to_value(self.get(&caller, id)?)?),
            TaskOperation::Cancel { id } => {
                let mut record = self.get(&caller, id)?;
                if matches!(record.state, TaskState::Pending | TaskState::Running) {
                    record.state = TaskState::Cancelled;
                    record.error = Some("cancelled by owner".into());
                    self.save(&record)?;
                }
                Ok(serde_json::to_value(record)?)
            }
            TaskOperation::KnowledgePut { key, value } => {
                let key = knowledge_key(&caller, &key)?;
                let bytes = serde_json::to_vec(&value)?;
                ensure!(bytes.len() <= MAX_ARGS, "knowledge value exceeds 64 KiB");
                if let Some(existing) = self.backend.get(KNOWLEDGE, &key)? {
                    ensure!(
                        existing == bytes,
                        "knowledge key already holds different content; use a new versioned key"
                    );
                    return Ok(serde_json::json!({"stored":true}));
                }
                ensure!(
                    self.backend.exists(KNOWLEDGE, &key)?
                        || self.backend.count(KNOWLEDGE)? < MAX_TASKS,
                    "knowledge store capacity reached"
                );
                self.backend.put(KNOWLEDGE, &key, &bytes)?;
                Ok(serde_json::json!({"stored":true}))
            }
            TaskOperation::KnowledgeGet { key } => {
                let bytes = self
                    .backend
                    .get(KNOWLEDGE, &knowledge_key(&caller, &key)?)?
                    .context("knowledge not found for this owner")?;
                Ok(serde_json::from_slice(&bytes)?)
            }
        }
    }

    fn next_ready(&self) -> Result<Option<TaskRecord>> {
        let _lock = self.writes.lock().unwrap();
        let keys: Vec<_> = self.pending.lock().unwrap().iter().cloned().collect();
        for key in keys {
            let bytes = self
                .backend
                .get(TASKS, &key)?
                .context("pending task disappeared")?;
            let mut record: TaskRecord = serde_json::from_slice(&bytes)?;
            if record.state != TaskState::Pending || !self.allowed.contains(&record.owner) {
                continue;
            }
            if now().saturating_sub(record.created_at) >= u64::from(record.spec.timeout_secs) {
                record.state = TaskState::Failed;
                record.error = Some("task deadline exceeded".into());
                self.save(&record)?;
                continue;
            }
            let dependencies: Vec<_> = record
                .spec
                .dependencies
                .iter()
                .map(|id| self.get(&record.owner, *id))
                .collect::<Result<_>>()?;
            if dependencies
                .iter()
                .any(|r| matches!(r.state, TaskState::Failed | TaskState::Cancelled))
            {
                record.state = TaskState::Failed;
                record.error = Some("dependency failed or was cancelled".into());
                self.save(&record)?;
                continue;
            }
            if dependencies.iter().any(|r| r.state != TaskState::Completed) {
                continue;
            }
            if let Some(input) = &record.spec.input_from {
                let dependency = self.get(&record.owner, input.task_id)?;
                let content = match dependency.result.as_ref().map(|r| &r.outcome) {
                    Some(ToolOutcome::Ok { content }) => content.pointer(&input.pointer).cloned(),
                    _ => None,
                };
                let Some(content) = content else {
                    record.state = TaskState::Failed;
                    record.error = Some("dependency input pointer did not resolve".into());
                    self.save(&record)?;
                    continue;
                };
                // Keep the immutable submitted specification unchanged for
                // duplicate detection; only the returned execution copy changes.
                record.state = TaskState::Running;
                self.save(&record)?;
                record.spec.args["text"] = content;
            } else {
                record.state = TaskState::Running;
                self.save(&record)?;
            }
            return Ok(Some(record));
        }
        Ok(None)
    }

    pub async fn tick(&self) -> Result<()> {
        for _ in 0..16 {
            let Some(record) = self.next_ready()? else {
                break;
            };
            let call = ToolCall {
                id: record.spec.id.to_string(),
                name: record.spec.tool.clone(),
                args: record.spec.args,
                trace: None,
            };
            let remaining = u64::from(record.spec.timeout_secs)
                .saturating_sub(now().saturating_sub(record.created_at))
                .max(1);
            let result = tokio::time::timeout(
                Duration::from_secs(remaining),
                tokio::task::spawn_blocking(move || crate::tools::invoke(call)),
            )
            .await;
            let _lock = self.writes.lock().unwrap();
            let mut current = self.get(&record.owner, record.spec.id)?;
            // Owner cancellation wins even if a blocking read-only parser has
            // just finished; its output never replaces the cancelled state.
            if current.state != TaskState::Running {
                continue;
            }
            if now().saturating_sub(current.created_at) >= u64::from(current.spec.timeout_secs) {
                current.state = TaskState::Failed;
                current.error = Some("task deadline exceeded".into());
                self.save(&current)?;
                continue;
            }
            match result {
                Ok(Ok(result)) if serde_json::to_vec(&result)?.len() <= MAX_RESULT => {
                    current.state = if matches!(result.outcome, ToolOutcome::Ok { .. }) {
                        TaskState::Completed
                    } else {
                        TaskState::Failed
                    };
                    current.result = Some(result);
                }
                _ => {
                    current.state = TaskState::Failed;
                    current.error =
                        Some("tool deadline, output bound, or execution failure".into());
                }
            }
            self.save(&current)?;
        }
        Ok(())
    }

    pub fn start(self: &Arc<Self>) {
        let service = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(100));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                if let Err(error) = service.tick().await {
                    tracing::warn!(%error,"task worker pass failed");
                }
            }
        });
    }
}

#[async_trait::async_trait]
impl spine_agentic::naming_mesh::MeshApplication for TaskService {
    async fn handle(&self, caller: [u8; 32], request: serde_json::Value) -> serde_json::Value {
        if !self.allowed.contains(&caller) {
            return serde_json::json!({"error":{"code":"not_authorized","message":"peer signing key is not authorized for tasks"}});
        }
        let result = serde_json::from_value(request)
            .map_err(anyhow::Error::from)
            .and_then(|op| self.request(caller, op));
        match result {
            Ok(value) => serde_json::json!({"result":value}),
            Err(error) => {
                serde_json::json!({"error":{"code":"task_rejected","message":error.to_string()}})
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spine_protocol::tasks::TaskInput;
    fn spec() -> TaskSpec {
        TaskSpec {
            id: Uuid::new_v4(),
            tool: "spine.content/hash".into(),
            args: serde_json::json!({"text":"abc"}),
            dependencies: vec![],
            input_from: None,
            timeout_secs: 300,
        }
    }
    #[tokio::test]
    async fn durable_dependencies_duplicates_cancellation_and_owner_isolation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tasks.sqlite");
        let open = || {
            TaskService::new(
                Arc::new(
                    spine_storage::sqlite::SqliteBackend::open(path.to_str().unwrap()).unwrap(),
                ),
                vec![[1; 32], [2; 32]],
            )
            .unwrap()
        };
        let service = open();
        let first = spec();
        let mut second = spec();
        second.dependencies = vec![first.id];
        second.input_from = Some(TaskInput {
            task_id: first.id,
            pointer: "/sha256".into(),
        });
        service
            .request(
                [1; 32],
                TaskOperation::Submit {
                    spec: first.clone(),
                },
            )
            .unwrap();
        service
            .request(
                [1; 32],
                TaskOperation::Submit {
                    spec: second.clone(),
                },
            )
            .unwrap();
        assert!(service
            .request([2; 32], TaskOperation::Status { id: first.id })
            .is_err());
        assert!(service
            .request([3; 32], TaskOperation::Status { id: first.id })
            .is_err());
        let mut conflicting = first.clone();
        conflicting.args = serde_json::json!({"text":"different"});
        assert!(service
            .request([1; 32], TaskOperation::Submit { spec: conflicting })
            .is_err());
        service.tick().await.unwrap();
        assert_eq!(
            service.get(&[1; 32], second.id).unwrap().state,
            TaskState::Completed
        );
        service
            .request(
                [1; 32],
                TaskOperation::Submit {
                    spec: second.clone(),
                },
            )
            .unwrap();
        let cancelled = spec();
        let mut dependent = spec();
        dependent.dependencies = vec![cancelled.id];
        service
            .request(
                [1; 32],
                TaskOperation::Submit {
                    spec: cancelled.clone(),
                },
            )
            .unwrap();
        service
            .request(
                [1; 32],
                TaskOperation::Submit {
                    spec: dependent.clone(),
                },
            )
            .unwrap();
        service
            .request([1; 32], TaskOperation::Cancel { id: cancelled.id })
            .unwrap();
        service.tick().await.unwrap();
        assert_eq!(
            service.get(&[1; 32], dependent.id).unwrap().state,
            TaskState::Failed
        );
        let recovered = spec();
        service
            .request(
                [1; 32],
                TaskOperation::Submit {
                    spec: recovered.clone(),
                },
            )
            .unwrap();
        {
            let mut record = service.get(&[1; 32], recovered.id).unwrap();
            record.state = TaskState::Running;
            service.save(&record).unwrap();
        }
        drop(service);
        let service = open();
        assert_eq!(
            service.get(&[1; 32], recovered.id).unwrap().state,
            TaskState::Pending
        );
        service.tick().await.unwrap();
        assert_eq!(
            service.get(&[1; 32], recovered.id).unwrap().state,
            TaskState::Completed
        );
        service
            .request(
                [1; 32],
                TaskOperation::KnowledgePut {
                    key: "artifact".into(),
                    value: serde_json::json!({"hash":"test"}),
                },
            )
            .unwrap();
        assert!(service
            .request(
                [2; 32],
                TaskOperation::KnowledgeGet {
                    key: "artifact".into()
                }
            )
            .is_err());
        assert!(
            service
                .request(
                    [1; 32],
                    TaskOperation::KnowledgePut {
                        key: "artifact".into(),
                        value: serde_json::json!({"hash":"replacement"})
                    }
                )
                .is_err(),
            "replayed publication must never replace immutable knowledge"
        );
        service
            .request(
                [1; 32],
                TaskOperation::KnowledgePut {
                    key: "artifact".into(),
                    value: serde_json::json!({"hash":"test"}),
                },
            )
            .unwrap();
        drop(service);
        assert_eq!(
            open()
                .request(
                    [1; 32],
                    TaskOperation::KnowledgeGet {
                        key: "artifact".into()
                    }
                )
                .unwrap()["hash"],
            "test"
        );
    }
}
