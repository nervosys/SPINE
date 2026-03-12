// =============================================================================
// SPINE Distributed Task Scheduler with Work-Stealing
// =============================================================================
//
// Distributes tasks across mesh nodes with work-stealing for load balancing.
// Integrates with the lifecycle manager for agent-aware scheduling and
// with the mesh for cross-node task redistribution.
//
// =============================================================================

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::cmp::Ordering;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::AgentId;

// =============================================================================
// TASK DEFINITIONS
// =============================================================================

/// Unique identifier for a task.
pub type TaskId = Uuid;

/// Priority level for task scheduling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TaskPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

impl PartialOrd for TaskPriority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TaskPriority {
    fn cmp(&self, other: &Self) -> Ordering {
        (*self as u8).cmp(&(*other as u8))
    }
}

/// The current status of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TaskStatus {
    /// Waiting in the queue.
    Queued,
    /// Assigned to a worker and running.
    Running,
    /// Completed successfully.
    Completed,
    /// Failed with an error.
    Failed,
    /// Cancelled before completion.
    Cancelled,
    /// Stolen from another node's queue.
    Stolen,
}

/// A schedulable task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: TaskId,
    pub name: String,
    pub description: String,
    pub priority: TaskPriority,
    pub status: TaskStatus,
    /// Which agent should run this (if any). None = any capable agent.
    pub assigned_agent: Option<AgentId>,
    /// Required capabilities for the executing agent.
    pub required_capabilities: Vec<String>,
    /// Task payload (serialized task-specific data).
    pub payload: serde_json::Value,
    /// Dependencies: tasks that must complete before this one starts.
    pub dependencies: Vec<TaskId>,
    /// Maximum attempts before marking as permanently failed.
    pub max_retries: u32,
    /// Current attempt number.
    pub attempt: u32,
    /// Deadline: if set, the task should complete before this time.
    pub deadline: Option<DateTime<Utc>>,
    /// The node this task is queued/running on.
    pub node_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

impl Task {
    pub fn new(name: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            description: String::new(),
            priority: TaskPriority::Normal,
            status: TaskStatus::Queued,
            assigned_agent: None,
            required_capabilities: Vec::new(),
            payload,
            dependencies: Vec::new(),
            max_retries: 3,
            attempt: 0,
            deadline: None,
            node_id: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
        }
    }

    pub fn with_priority(mut self, priority: TaskPriority) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_agent(mut self, agent_id: AgentId) -> Self {
        self.assigned_agent = Some(agent_id);
        self
    }

    pub fn with_deadline(mut self, deadline: DateTime<Utc>) -> Self {
        self.deadline = Some(deadline);
        self
    }

    pub fn with_dependencies(mut self, deps: Vec<TaskId>) -> Self {
        self.dependencies = deps;
        self
    }

    pub fn with_capabilities(mut self, caps: Vec<String>) -> Self {
        self.required_capabilities = caps;
        self
    }

    /// Whether all dependencies have been met.
    pub fn dependencies_met(&self, completed: &std::collections::HashSet<TaskId>) -> bool {
        self.dependencies.iter().all(|dep| completed.contains(dep))
    }

    /// Whether the task has exceeded its deadline.
    pub fn is_overdue(&self) -> bool {
        self.deadline.map(|d| Utc::now() > d).unwrap_or(false)
    }
}

/// A wrapper for priority-queue ordering (max-heap by priority, then earliest deadline).
#[derive(Debug, Clone)]
struct PrioritizedTask {
    task: Task,
}

impl PartialEq for PrioritizedTask {
    fn eq(&self, other: &Self) -> bool {
        self.task.id == other.task.id
    }
}

impl Eq for PrioritizedTask {}

impl PartialOrd for PrioritizedTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrioritizedTask {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher priority first
        self.task.priority.cmp(&other.task.priority)
            .then_with(|| {
                // Earlier deadline first (reversed so earlier = greater)
                match (self.task.deadline, other.task.deadline) {
                    (Some(a), Some(b)) => b.cmp(&a), // earlier deadline = higher priority
                    (Some(_), None) => Ordering::Greater,
                    (None, Some(_)) => Ordering::Less,
                    (None, None) => self.task.created_at.cmp(&other.task.created_at).reverse(),
                }
            })
    }
}

// =============================================================================
// TASK RESULT
// =============================================================================

/// The result of executing a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub task_id: TaskId,
    pub status: TaskStatus,
    pub output: Option<serde_json::Value>,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub completed_at: DateTime<Utc>,
}

// =============================================================================
// WORK-STEALING QUEUE
// =============================================================================

/// A per-node work queue that supports stealing from the back.
#[derive(Debug)]
pub struct WorkQueue {
    /// Priority queue for local task scheduling.
    queue: BinaryHeap<PrioritizedTask>,
    /// Stealable deque: other nodes steal from here.
    stealable: VecDeque<Task>,
}

impl Default for WorkQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkQueue {
    pub fn new() -> Self {
        Self {
            queue: BinaryHeap::new(),
            stealable: VecDeque::new(),
        }
    }

    /// Push a task into the queue.
    pub fn push(&mut self, task: Task) {
        self.stealable.push_back(task.clone());
        self.queue.push(PrioritizedTask { task });
    }

    /// Pop the highest-priority task for local execution.
    pub fn pop(&mut self) -> Option<Task> {
        if let Some(pt) = self.queue.pop() {
            // Remove from stealable deque
            self.stealable.retain(|t| t.id != pt.task.id);
            Some(pt.task)
        } else {
            None
        }
    }

    /// Steal a task from the back of the queue (for work-stealing).
    /// Returns the lowest-priority stealable task.
    pub fn steal(&mut self) -> Option<Task> {
        if let Some(stolen) = self.stealable.pop_front() {
            // Remove from priority queue by rebuilding without it
            let id = stolen.id;
            let remaining: Vec<_> = self.queue.drain().filter(|pt| pt.task.id != id).collect();
            self.queue = remaining.into_iter().collect();
            Some(stolen)
        } else {
            None
        }
    }

    /// Number of tasks in the queue.
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

// =============================================================================
// TASK SCHEDULER
// =============================================================================

/// Configuration for the task scheduler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    /// Threshold: if a node has more than this many tasks, it's considered overloaded.
    pub overload_threshold: usize,
    /// Threshold: if a node has fewer than this many tasks, it should steal work.
    pub steal_threshold: usize,
    /// Maximum tasks to steal at once.
    pub max_steal_batch: usize,
    /// How often to check for rebalancing (seconds).
    pub rebalance_interval_secs: u64,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            overload_threshold: 50,
            steal_threshold: 5,
            max_steal_batch: 10,
            rebalance_interval_secs: 10,
        }
    }
}

/// Distributed task scheduler with work-stealing.
pub struct TaskScheduler {
    node_id: Uuid,
    config: SchedulerConfig,
    local_queue: Arc<RwLock<WorkQueue>>,
    tasks: DashMap<TaskId, Task>,
    results: DashMap<TaskId, TaskResult>,
    completed_set: Arc<RwLock<std::collections::HashSet<TaskId>>>,
    /// Known peer node queue depths for stealing decisions.
    peer_depths: DashMap<Uuid, usize>,
}

impl TaskScheduler {
    pub fn new(node_id: Uuid, config: SchedulerConfig) -> Self {
        Self {
            node_id,
            config,
            local_queue: Arc::new(RwLock::new(WorkQueue::new())),
            tasks: DashMap::new(),
            results: DashMap::new(),
            completed_set: Arc::new(RwLock::new(std::collections::HashSet::new())),
            peer_depths: DashMap::new(),
        }
    }

    /// Submit a task to the scheduler.
    pub async fn submit(&self, mut task: Task) -> TaskId {
        task.node_id = Some(self.node_id);
        let id = task.id;
        self.tasks.insert(id, task.clone());
        self.local_queue.write().await.push(task);
        id
    }

    /// Dequeue the next ready task for execution.
    pub async fn dequeue(&self) -> Option<Task> {
        let completed = self.completed_set.read().await;
        let mut queue = self.local_queue.write().await;

        // Try to find a task whose dependencies are all met
        let mut deferred = Vec::new();
        let result = loop {
            match queue.pop() {
                Some(task) if task.dependencies_met(&completed) => break Some(task),
                Some(task) => deferred.push(task),
                None => break None,
            }
        };

        // Re-enqueue deferred tasks
        for task in deferred {
            queue.push(task);
        }

        if let Some(ref task) = result {
            if let Some(mut t) = self.tasks.get_mut(&task.id) {
                t.status = TaskStatus::Running;
                t.started_at = Some(Utc::now());
                t.attempt += 1;
            }
        }

        result
    }

    /// Complete a task with a result.
    pub async fn complete(&self, task_id: TaskId, output: Option<serde_json::Value>) {
        let now = Utc::now();
        if let Some(mut task) = self.tasks.get_mut(&task_id) {
            task.status = TaskStatus::Completed;
            task.completed_at = Some(now);
        }

        let started = self
            .tasks
            .get(&task_id)
            .and_then(|t| t.started_at)
            .unwrap_or(now);
        let duration_ms = (now - started).num_milliseconds().unsigned_abs();

        let result = TaskResult {
            task_id,
            status: TaskStatus::Completed,
            output,
            error: None,
            duration_ms,
            completed_at: now,
        };
        self.results.insert(task_id, result);
        self.completed_set.write().await.insert(task_id);
    }

    /// Fail a task. If retries remain, re-enqueue it; otherwise mark as failed.
    pub async fn fail(&self, task_id: TaskId, error: String) {
        let should_retry = self.tasks.get(&task_id).map(|t| {
            t.attempt < t.max_retries
        }).unwrap_or(false);

        if should_retry {
            if let Some(mut task) = self.tasks.get_mut(&task_id) {
                task.status = TaskStatus::Queued;
                task.started_at = None;
                let requeue = task.clone();
                drop(task);
                self.local_queue.write().await.push(requeue);
            }
        } else {
            let now = Utc::now();
            if let Some(mut task) = self.tasks.get_mut(&task_id) {
                task.status = TaskStatus::Failed;
                task.completed_at = Some(now);
            }
            let result = TaskResult {
                task_id,
                status: TaskStatus::Failed,
                output: None,
                error: Some(error),
                duration_ms: 0,
                completed_at: now,
            };
            self.results.insert(task_id, result);
        }
    }

    /// Cancel a queued task.
    pub async fn cancel(&self, task_id: TaskId) -> bool {
        if let Some(mut task) = self.tasks.get_mut(&task_id) {
            if task.status == TaskStatus::Queued {
                task.status = TaskStatus::Cancelled;
                return true;
            }
        }
        false
    }

    /// Steal tasks from this node (called by a remote thief node).
    pub async fn steal_tasks(&self, max: usize) -> Vec<Task> {
        let mut queue = self.local_queue.write().await;
        let mut stolen = Vec::new();
        for _ in 0..max {
            match queue.steal() {
                Some(mut task) => {
                    task.status = TaskStatus::Stolen;
                    if let Some(mut t) = self.tasks.get_mut(&task.id) {
                        t.status = TaskStatus::Stolen;
                    }
                    stolen.push(task);
                }
                None => break,
            }
        }
        stolen
    }

    /// Accept stolen tasks onto this node.
    pub async fn accept_stolen(&self, tasks: Vec<Task>) {
        let mut queue = self.local_queue.write().await;
        for mut task in tasks {
            task.status = TaskStatus::Queued;
            task.node_id = Some(self.node_id);
            self.tasks.insert(task.id, task.clone());
            queue.push(task);
        }
    }

    /// Update known depth of a peer's queue (for steal decisions).
    pub fn update_peer_depth(&self, peer_id: Uuid, depth: usize) {
        self.peer_depths.insert(peer_id, depth);
    }

    /// Determine which peer to steal from (the most loaded one).
    pub fn pick_steal_target(&self) -> Option<Uuid> {
        self.peer_depths
            .iter()
            .filter(|entry| *entry.value() > self.config.overload_threshold)
            .max_by_key(|entry| *entry.value())
            .map(|entry| *entry.key())
    }

    /// Whether this node should try to steal work.
    pub async fn should_steal(&self) -> bool {
        let depth = self.local_queue.read().await.len();
        depth < self.config.steal_threshold
    }

    /// Get task status.
    pub fn get_task(&self, task_id: &TaskId) -> Option<Task> {
        self.tasks.get(task_id).map(|t| t.clone())
    }

    /// Get task result.
    pub fn get_result(&self, task_id: &TaskId) -> Option<TaskResult> {
        self.results.get(task_id).map(|r| r.clone())
    }

    /// Local queue depth.
    pub async fn queue_depth(&self) -> usize {
        self.local_queue.read().await.len()
    }

    /// Total tasks managed by this scheduler.
    pub fn total_tasks(&self) -> usize {
        self.tasks.len()
    }

    /// Get scheduler stats.
    pub async fn stats(&self) -> SchedulerStats {
        let queue_depth = self.local_queue.read().await.len();
        let mut by_status = HashMap::new();
        for entry in self.tasks.iter() {
            *by_status.entry(entry.value().status).or_insert(0usize) += 1;
        }
        SchedulerStats {
            node_id: self.node_id,
            queue_depth,
            total_tasks: self.tasks.len(),
            completed: self.results.len(),
            by_status,
        }
    }
}

/// Scheduler statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerStats {
    pub node_id: Uuid,
    pub queue_depth: usize,
    pub total_tasks: usize,
    pub completed: usize,
    pub by_status: HashMap<TaskStatus, usize>,
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_creation() {
        let task = Task::new("test", serde_json::json!({"key": "value"}))
            .with_priority(TaskPriority::High);
        assert_eq!(task.priority, TaskPriority::High);
        assert_eq!(task.status, TaskStatus::Queued);
        assert_eq!(task.attempt, 0);
    }

    #[test]
    fn test_task_priority_ordering() {
        assert!(TaskPriority::Critical > TaskPriority::High);
        assert!(TaskPriority::High > TaskPriority::Normal);
        assert!(TaskPriority::Normal > TaskPriority::Low);
    }

    #[test]
    fn test_work_queue_priority_ordering() {
        let mut queue = WorkQueue::new();

        queue.push(Task::new("low", serde_json::json!(null)).with_priority(TaskPriority::Low));
        queue.push(Task::new("critical", serde_json::json!(null)).with_priority(TaskPriority::Critical));
        queue.push(Task::new("normal", serde_json::json!(null)).with_priority(TaskPriority::Normal));

        assert_eq!(queue.pop().unwrap().name, "critical");
        assert_eq!(queue.pop().unwrap().name, "normal");
        assert_eq!(queue.pop().unwrap().name, "low");
    }

    #[test]
    fn test_work_queue_steal() {
        let mut queue = WorkQueue::new();
        queue.push(Task::new("first", serde_json::json!(null)).with_priority(TaskPriority::Low));
        queue.push(Task::new("second", serde_json::json!(null)).with_priority(TaskPriority::High));

        // Steal takes from the front (earliest added, typically lowest priority)
        let stolen = queue.steal().unwrap();
        assert_eq!(stolen.name, "first");
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn test_work_queue_empty() {
        let mut queue = WorkQueue::new();
        assert!(queue.is_empty());
        assert!(queue.pop().is_none());
        assert!(queue.steal().is_none());
    }

    #[tokio::test]
    async fn test_scheduler_submit_and_dequeue() {
        let scheduler = TaskScheduler::new(Uuid::new_v4(), SchedulerConfig::default());
        let task = Task::new("test-task", serde_json::json!({"x": 1}));
        let task_id = scheduler.submit(task).await;

        let dequeued = scheduler.dequeue().await.unwrap();
        assert_eq!(dequeued.id, task_id);
        assert_eq!(scheduler.get_task(&task_id).unwrap().status, TaskStatus::Running);
    }

    #[tokio::test]
    async fn test_scheduler_complete() {
        let scheduler = TaskScheduler::new(Uuid::new_v4(), SchedulerConfig::default());
        let task_id = scheduler.submit(Task::new("t", serde_json::json!(null))).await;
        scheduler.dequeue().await;
        scheduler.complete(task_id, Some(serde_json::json!({"result": "ok"}))).await;

        let result = scheduler.get_result(&task_id).unwrap();
        assert_eq!(result.status, TaskStatus::Completed);
        assert!(result.output.is_some());
    }

    #[tokio::test]
    async fn test_scheduler_fail_with_retry() {
        let scheduler = TaskScheduler::new(Uuid::new_v4(), SchedulerConfig::default());
        let mut task = Task::new("retry-task", serde_json::json!(null));
        task.max_retries = 2;
        let task_id = scheduler.submit(task).await;

        // First attempt
        scheduler.dequeue().await;
        scheduler.fail(task_id, "oops".to_string()).await;

        // Should be re-enqueued
        assert_eq!(scheduler.get_task(&task_id).unwrap().status, TaskStatus::Queued);

        // Second attempt
        scheduler.dequeue().await;
        scheduler.fail(task_id, "again".to_string()).await;

        // Should now be permanently failed (2 attempts = max_retries)
        assert_eq!(scheduler.get_task(&task_id).unwrap().status, TaskStatus::Failed);
    }

    #[tokio::test]
    async fn test_scheduler_dependency_ordering() {
        let scheduler = TaskScheduler::new(Uuid::new_v4(), SchedulerConfig::default());

        let dep_task = Task::new("dependency", serde_json::json!(null));
        let dep_id = dep_task.id;
        scheduler.submit(dep_task).await;

        let dependent = Task::new("dependent", serde_json::json!(null))
            .with_dependencies(vec![dep_id]);
        let dependent_id = scheduler.submit(dependent).await;

        // Should get the dependency first
        let first = scheduler.dequeue().await.unwrap();
        assert_eq!(first.id, dep_id);

        // Dependent should NOT be dequeued yet (dependency not completed)
        let second = scheduler.dequeue().await;
        assert!(second.is_none());

        // Complete the dependency
        scheduler.complete(dep_id, None).await;

        // Now the dependent task should be available
        let third = scheduler.dequeue().await.unwrap();
        assert_eq!(third.id, dependent_id);
    }

    #[tokio::test]
    async fn test_scheduler_steal() {
        let node_a = Uuid::new_v4();
        let node_b = Uuid::new_v4();
        let scheduler_a = TaskScheduler::new(node_a, SchedulerConfig::default());
        let scheduler_b = TaskScheduler::new(node_b, SchedulerConfig::default());

        // Load up node A
        for i in 0..5 {
            scheduler_a.submit(Task::new(format!("task-{}", i), serde_json::json!(null))).await;
        }

        // Steal from A
        let stolen = scheduler_a.steal_tasks(2).await;
        assert_eq!(stolen.len(), 2);

        // Accept on B
        scheduler_b.accept_stolen(stolen).await;
        assert_eq!(scheduler_b.queue_depth().await, 2);
        assert_eq!(scheduler_a.queue_depth().await, 3);
    }

    #[tokio::test]
    async fn test_scheduler_cancel() {
        let scheduler = TaskScheduler::new(Uuid::new_v4(), SchedulerConfig::default());
        let task_id = scheduler.submit(Task::new("cancel-me", serde_json::json!(null))).await;
        assert!(scheduler.cancel(task_id).await);
        assert_eq!(scheduler.get_task(&task_id).unwrap().status, TaskStatus::Cancelled);
    }

    #[tokio::test]
    async fn test_scheduler_stats() {
        let scheduler = TaskScheduler::new(Uuid::new_v4(), SchedulerConfig::default());
        scheduler.submit(Task::new("a", serde_json::json!(null))).await;
        scheduler.submit(Task::new("b", serde_json::json!(null))).await;

        let stats = scheduler.stats().await;
        assert_eq!(stats.queue_depth, 2);
        assert_eq!(stats.total_tasks, 2);
    }

    #[test]
    fn test_pick_steal_target() {
        let scheduler = TaskScheduler::new(Uuid::new_v4(), SchedulerConfig::default());
        let overloaded = Uuid::new_v4();
        let idle = Uuid::new_v4();

        scheduler.update_peer_depth(overloaded, 100);
        scheduler.update_peer_depth(idle, 2);

        let target = scheduler.pick_steal_target().unwrap();
        assert_eq!(target, overloaded);
    }
}
