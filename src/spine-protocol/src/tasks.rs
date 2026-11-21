//! Durable, idempotent task contracts for the signed mesh application surface.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TaskInput {
    pub task_id: Uuid,
    /// JSON pointer into the dependency's successful result content.
    pub pointer: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TaskSpec {
    pub id: Uuid,
    pub tool: String,
    pub args: serde_json::Value,
    #[serde(default)]
    pub dependencies: Vec<Uuid>,
    #[serde(default)]
    pub input_from: Option<TaskInput>,
    /// Deadline from submission, including dependency waiting time.
    pub timeout_secs: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TaskState {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRecord {
    pub owner: [u8; 32],
    pub spec: TaskSpec,
    pub state: TaskState,
    pub created_at: u64,
    /// Unix seconds at which the task first became terminal (`Completed`,
    /// `Failed` or `Cancelled`); `None` while pending or running, and for
    /// records written before this field existed. The worker uses it to order
    /// eviction, so a result is not evicted ahead of older finished work just
    /// because the task was submitted long ago.
    #[serde(default)]
    pub completed_at: Option<u64>,
    pub result: Option<crate::ToolResult>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskOperation {
    Capabilities,
    Submit {
        spec: TaskSpec,
    },
    Status {
        id: Uuid,
    },
    Cancel {
        id: Uuid,
    },
    KnowledgePut {
        key: String,
        value: serde_json::Value,
    },
    KnowledgeGet {
        key: String,
    },
}
