// Allow dead code for extensive public API surface designed for future use
#![allow(dead_code)]

//! # SPINE Recursive Language Models
//!
//! Implementation of Recursive Language Models (RLMs) for infinite context processing,
//! based on the paper "Recursive Language Models" (Zhang et al., 2025).
//!
//! ## Key Insight
//!
//! Long prompts should not be fed into the neural network directly but should
//! instead be treated as **part of the environment** that the LLM can symbolically
//! interact with.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                 RECURSIVE LANGUAGE MODEL                        │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  ┌─────────────────────────────────────────────────────────────┐│
//! │  │                      REPL Environment                       ││
//! │  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐         ││
//! │  │  │ Context Var │  │ Code Exec   │  │ Sub-LLM     │         ││
//! │  │  │ (Infinite)  │  │ Engine      │  │ Dispatcher  │         ││
//! │  │  └─────────────┘  └─────────────┘  └─────────────┘         ││
//! │  └─────────────────────────────────────────────────────────────┘│
//! │                              │                                  │
//! │  ┌───────────────────────────▼────────────────────────────────┐│
//! │  │               Recursive Sub-Call Stack                      ││
//! │  │  ┌─────┐  ┌─────┐  ┌─────┐  ┌─────┐                        ││
//! │  │  │Root │→ │Sub₁ │→ │Sub₂ │→ │...  │                        ││
//! │  │  │ LLM │  │ LLM │  │ LLM │  │     │                        ││
//! │  │  └─────┘  └─────┘  └─────┘  └─────┘                        ││
//! │  └─────────────────────────────────────────────────────────────┘│
//! │                              │                                  │
//! │  ┌───────────────────────────▼────────────────────────────────┐│
//! │  │               Context Management                            ││
//! │  │  • Chunking & Decomposition                                 ││
//! │  │  • Regex/Keyword Filtering                                  ││
//! │  │  • Semantic Transformation                                  ││
//! │  │  • Result Aggregation                                       ││
//! │  └─────────────────────────────────────────────────────────────┘│
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! ```rust,ignore
//! use spine_recursive::{RecursiveLM, ReplEnvironment, ContextVariable};
//!
//! // Create RLM with infinite context support
//! let rlm = RecursiveLM::new(model_config);
//!
//! // Load arbitrarily large context (10M+ tokens)
//! let context = load_massive_document();
//! rlm.load_context(context).await?;
//!
//! // Query - RLM handles decomposition automatically
//! let answer = rlm.query("Find all references to quantum computing").await?;
//! ```

// Production LLM dispatchers (OpenAI, Anthropic, etc.)
pub mod llm_dispatchers;

pub use llm_dispatchers::{
    AdaptiveDispatcher, AnthropicConfig, AnthropicDispatcher, LoadBalanceStrategy,
    LoadBalancedDispatcher, OfflineDispatcher, OpenAiConfig, OpenAiDispatcher,
};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::RwLock;
use uuid::Uuid;

// =============================================================================
// ERRORS
// =============================================================================

/// Errors that can occur during RLM operations
#[derive(Error, Debug)]
pub enum RlmError {
    #[error("Context too large: {size} bytes exceeds limit")]
    ContextTooLarge { size: usize },

    #[error("Recursion depth exceeded: {depth} > {max_depth}")]
    RecursionDepthExceeded { depth: usize, max_depth: usize },

    #[error("Sub-LLM call failed: {reason}")]
    SubLlmCallFailed { reason: String },

    #[error("Code execution failed: {reason}")]
    CodeExecutionFailed { reason: String },

    #[error("Context variable not found: {name}")]
    VariableNotFound { name: String },

    #[error("Invalid chunk index: {index} for context of {chunks} chunks")]
    InvalidChunkIndex { index: usize, chunks: usize },

    #[error("Timeout after {duration:?}")]
    Timeout { duration: Duration },

    #[error("Model error: {message}")]
    ModelError { message: String },

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, RlmError>;

// =============================================================================
// CORE TYPES
// =============================================================================

/// Unique identifier for RLM sessions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub Uuid);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

/// Unique identifier for context chunks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChunkId(pub u64);

impl ChunkId {
    pub fn new(index: u64) -> Self {
        Self(index)
    }
}

/// Context variable stored in the REPL environment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextVariable {
    /// Variable name
    pub name: String,
    /// Full content (can be massive)
    pub content: String,
    /// Total character count
    pub total_chars: usize,
    /// Chunk boundaries for efficient access
    pub chunk_boundaries: Vec<usize>,
    /// Chunk sizes for metadata
    pub chunk_sizes: Vec<usize>,
    /// Content type hint
    pub content_type: ContentType,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
}

impl ContextVariable {
    /// Create a new context variable with automatic chunking
    pub fn new(name: &str, content: String, chunk_size: usize) -> Self {
        let total_chars = content.len();
        let mut chunk_boundaries = vec![0];
        let mut chunk_sizes = Vec::new();

        let mut pos = 0;
        while pos < total_chars {
            let end = (pos + chunk_size).min(total_chars);
            // Try to break at newline for cleaner chunks
            let actual_end = if end < total_chars {
                content[pos..end]
                    .rfind('\n')
                    .map(|i| pos + i + 1)
                    .unwrap_or(end)
            } else {
                end
            };
            chunk_boundaries.push(actual_end);
            chunk_sizes.push(actual_end - pos);
            pos = actual_end;
        }

        Self {
            name: name.to_string(),
            content,
            total_chars,
            chunk_boundaries,
            chunk_sizes,
            content_type: ContentType::Text,
            created_at: Utc::now(),
        }
    }

    /// Get a specific chunk by index
    pub fn get_chunk(&self, index: usize) -> Option<&str> {
        if index + 1 >= self.chunk_boundaries.len() {
            return None;
        }
        let start = self.chunk_boundaries[index];
        let end = self.chunk_boundaries[index + 1];
        Some(&self.content[start..end])
    }

    /// Get number of chunks
    pub fn num_chunks(&self) -> usize {
        self.chunk_boundaries.len().saturating_sub(1)
    }

    /// Get slice of content by character range
    pub fn get_range(&self, start: usize, end: usize) -> Option<&str> {
        if end <= self.total_chars && start < end {
            Some(&self.content[start..end])
        } else {
            None
        }
    }

    /// Search content using regex and return matching chunks
    pub fn search_regex(&self, pattern: &str) -> Result<Vec<(usize, String)>> {
        let re = Regex::new(pattern).map_err(|e| RlmError::CodeExecutionFailed {
            reason: format!("Invalid regex: {}", e),
        })?;

        let mut results = Vec::new();
        for (i, chunk) in (0..self.num_chunks()).filter_map(|i| self.get_chunk(i).map(|c| (i, c))) {
            if re.is_match(chunk) {
                results.push((i, chunk.to_string()));
            }
        }
        Ok(results)
    }

    /// Search for keyword and return matching chunks
    pub fn search_keyword(&self, keyword: &str) -> Vec<(usize, String)> {
        let keyword_lower = keyword.to_lowercase();
        (0..self.num_chunks())
            .filter_map(|i| {
                self.get_chunk(i).and_then(|chunk| {
                    if chunk.to_lowercase().contains(&keyword_lower) {
                        Some((i, chunk.to_string()))
                    } else {
                        None
                    }
                })
            })
            .collect()
    }

    /// Get lines from content
    pub fn get_lines(&self, start_line: usize, end_line: usize) -> Vec<&str> {
        self.content
            .lines()
            .skip(start_line)
            .take(end_line - start_line)
            .collect()
    }

    /// Count lines in content
    pub fn line_count(&self) -> usize {
        self.content.lines().count()
    }
}

/// Type of content stored in context
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentType {
    Text,
    Json,
    Code,
    Markdown,
    Html,
    Csv,
    Documents,
}

// =============================================================================
// REPL ENVIRONMENT
// =============================================================================

/// Read-Eval-Print Loop environment for RLM context management
///
/// The REPL is the core abstraction that allows LLMs to interact with
/// arbitrarily large contexts through symbolic manipulation rather than
/// direct neural processing.
pub struct ReplEnvironment {
    /// Session identifier
    session_id: SessionId,
    /// Stored context variables
    variables: DashMap<String, Arc<ContextVariable>>,
    /// Execution history
    history: RwLock<Vec<ExecutionRecord>>,
    /// Sub-LLM call dispatcher
    sub_llm_dispatcher: Arc<dyn SubLlmDispatcher>,
    /// Maximum recursion depth
    max_recursion_depth: usize,
    /// Current recursion depth
    current_depth: AtomicUsize,
    /// Statistics
    stats: ReplStats,
}

/// Statistics for REPL operations
#[derive(Debug, Default)]
pub struct ReplStats {
    pub total_variables: AtomicUsize,
    pub total_chars_loaded: AtomicU64,
    pub sub_llm_calls: AtomicU64,
    pub code_executions: AtomicU64,
    pub regex_searches: AtomicU64,
    pub keyword_searches: AtomicU64,
    pub chunks_accessed: AtomicU64,
}

/// Record of an execution in the REPL
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionRecord {
    pub timestamp: DateTime<Utc>,
    pub operation: ReplOperation,
    pub input: String,
    pub output: String,
    pub duration_ms: u64,
    pub success: bool,
}

/// Types of operations in the REPL
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ReplOperation {
    LoadContext,
    GetChunk,
    SearchRegex,
    SearchKeyword,
    SubLlmCall,
    CodeExecution,
    GetVariable,
    SetVariable,
    GetLines,
    Aggregate,
}

impl ReplEnvironment {
    /// Create a new REPL environment
    pub fn new(dispatcher: Arc<dyn SubLlmDispatcher>, max_depth: usize) -> Self {
        Self {
            session_id: SessionId::new(),
            variables: DashMap::new(),
            history: RwLock::new(Vec::new()),
            sub_llm_dispatcher: dispatcher,
            max_recursion_depth: max_depth,
            current_depth: AtomicUsize::new(0),
            stats: ReplStats::default(),
        }
    }

    /// Load context as a variable
    pub async fn load_context(
        &self,
        name: &str,
        content: String,
        chunk_size: usize,
    ) -> Result<ContextMetadata> {
        let start = Instant::now();
        let total_chars = content.len();

        let var = ContextVariable::new(name, content, chunk_size);
        let num_chunks = var.num_chunks();
        let chunk_sizes = var.chunk_sizes.clone();

        let var = Arc::new(var);
        self.variables.insert(name.to_string(), var);

        self.stats.total_variables.fetch_add(1, Ordering::Relaxed);
        self.stats
            .total_chars_loaded
            .fetch_add(total_chars as u64, Ordering::Relaxed);

        self.record_execution(
            ReplOperation::LoadContext,
            name.to_string(),
            format!("Loaded {} chars in {} chunks", total_chars, num_chunks),
            start.elapsed(),
            true,
        )
        .await;

        Ok(ContextMetadata {
            name: name.to_string(),
            total_chars,
            num_chunks,
            chunk_sizes,
        })
    }

    /// Get a specific chunk from a variable
    pub async fn get_chunk(&self, var_name: &str, chunk_index: usize) -> Result<String> {
        let start = Instant::now();

        let var = self
            .variables
            .get(var_name)
            .ok_or_else(|| RlmError::VariableNotFound {
                name: var_name.to_string(),
            })?;

        let chunk = var
            .get_chunk(chunk_index)
            .ok_or_else(|| RlmError::InvalidChunkIndex {
                index: chunk_index,
                chunks: var.num_chunks(),
            })?
            .to_string();

        self.stats.chunks_accessed.fetch_add(1, Ordering::Relaxed);

        self.record_execution(
            ReplOperation::GetChunk,
            format!("{}[{}]", var_name, chunk_index),
            format!("{} chars", chunk.len()),
            start.elapsed(),
            true,
        )
        .await;

        Ok(chunk)
    }

    /// Search variable content using regex
    pub async fn search_regex(
        &self,
        var_name: &str,
        pattern: &str,
    ) -> Result<Vec<(usize, String)>> {
        let start = Instant::now();

        let var = self
            .variables
            .get(var_name)
            .ok_or_else(|| RlmError::VariableNotFound {
                name: var_name.to_string(),
            })?;

        let results = var.search_regex(pattern)?;

        self.stats.regex_searches.fetch_add(1, Ordering::Relaxed);

        self.record_execution(
            ReplOperation::SearchRegex,
            format!("{}.search('{}')", var_name, pattern),
            format!("{} matches", results.len()),
            start.elapsed(),
            true,
        )
        .await;

        Ok(results)
    }

    /// Search variable content using keyword
    pub async fn search_keyword(
        &self,
        var_name: &str,
        keyword: &str,
    ) -> Result<Vec<(usize, String)>> {
        let start = Instant::now();

        let var = self
            .variables
            .get(var_name)
            .ok_or_else(|| RlmError::VariableNotFound {
                name: var_name.to_string(),
            })?;

        let results = var.search_keyword(keyword);

        self.stats.keyword_searches.fetch_add(1, Ordering::Relaxed);

        self.record_execution(
            ReplOperation::SearchKeyword,
            format!("{}.search_keyword('{}')", var_name, keyword),
            format!("{} matches", results.len()),
            start.elapsed(),
            true,
        )
        .await;

        Ok(results)
    }

    /// Execute a sub-LLM call recursively
    pub async fn sub_llm_call(&self, prompt: &str, context_snippet: &str) -> Result<String> {
        let start = Instant::now();

        // Check recursion depth
        let depth = self.current_depth.fetch_add(1, Ordering::Relaxed);
        if depth >= self.max_recursion_depth {
            self.current_depth.fetch_sub(1, Ordering::Relaxed);
            return Err(RlmError::RecursionDepthExceeded {
                depth,
                max_depth: self.max_recursion_depth,
            });
        }

        let result = self
            .sub_llm_dispatcher
            .dispatch(prompt, context_snippet, depth)
            .await;

        self.current_depth.fetch_sub(1, Ordering::Relaxed);
        self.stats.sub_llm_calls.fetch_add(1, Ordering::Relaxed);

        let (output, success) = match &result {
            Ok(r) => (r.clone(), true),
            Err(e) => (e.to_string(), false),
        };

        self.record_execution(
            ReplOperation::SubLlmCall,
            format!("llm_query({} chars)", context_snippet.len()),
            output,
            start.elapsed(),
            success,
        )
        .await;

        result
    }

    /// Get lines from a variable
    pub async fn get_lines(
        &self,
        var_name: &str,
        start_line: usize,
        end_line: usize,
    ) -> Result<Vec<String>> {
        let start = Instant::now();

        let var = self
            .variables
            .get(var_name)
            .ok_or_else(|| RlmError::VariableNotFound {
                name: var_name.to_string(),
            })?;

        let lines: Vec<String> = var
            .get_lines(start_line, end_line)
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        self.record_execution(
            ReplOperation::GetLines,
            format!("{}[{}:{}]", var_name, start_line, end_line),
            format!("{} lines", lines.len()),
            start.elapsed(),
            true,
        )
        .await;

        Ok(lines)
    }

    /// Get variable metadata
    pub fn get_variable_info(&self, name: &str) -> Option<ContextMetadata> {
        self.variables.get(name).map(|var| ContextMetadata {
            name: var.name.clone(),
            total_chars: var.total_chars,
            num_chunks: var.num_chunks(),
            chunk_sizes: var.chunk_sizes.clone(),
        })
    }

    /// List all variables
    pub fn list_variables(&self) -> Vec<String> {
        self.variables.iter().map(|r| r.key().clone()).collect()
    }

    /// Get execution history
    pub async fn get_history(&self) -> Vec<ExecutionRecord> {
        self.history.read().await.clone()
    }

    /// Get statistics
    pub fn get_stats(&self) -> ReplStatsSnapshot {
        ReplStatsSnapshot {
            total_variables: self.stats.total_variables.load(Ordering::Relaxed),
            total_chars_loaded: self.stats.total_chars_loaded.load(Ordering::Relaxed),
            sub_llm_calls: self.stats.sub_llm_calls.load(Ordering::Relaxed),
            code_executions: self.stats.code_executions.load(Ordering::Relaxed),
            regex_searches: self.stats.regex_searches.load(Ordering::Relaxed),
            keyword_searches: self.stats.keyword_searches.load(Ordering::Relaxed),
            chunks_accessed: self.stats.chunks_accessed.load(Ordering::Relaxed),
        }
    }

    async fn record_execution(
        &self,
        operation: ReplOperation,
        input: String,
        output: String,
        duration: Duration,
        success: bool,
    ) {
        let record = ExecutionRecord {
            timestamp: Utc::now(),
            operation,
            input,
            output,
            duration_ms: duration.as_millis() as u64,
            success,
        };
        self.history.write().await.push(record);
    }
}

/// Metadata about a loaded context variable
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextMetadata {
    pub name: String,
    pub total_chars: usize,
    pub num_chunks: usize,
    pub chunk_sizes: Vec<usize>,
}

/// Snapshot of REPL statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplStatsSnapshot {
    pub total_variables: usize,
    pub total_chars_loaded: u64,
    pub sub_llm_calls: u64,
    pub code_executions: u64,
    pub regex_searches: u64,
    pub keyword_searches: u64,
    pub chunks_accessed: u64,
}

// =============================================================================
// SUB-LLM DISPATCHER
// =============================================================================

/// Trait for dispatching sub-LLM calls
#[async_trait]
pub trait SubLlmDispatcher: Send + Sync {
    /// Dispatch a sub-LLM call with the given prompt and context
    async fn dispatch(&self, prompt: &str, context: &str, depth: usize) -> Result<String>;

    /// Get the model identifier
    fn model_id(&self) -> &str;

    /// Get cost estimate for a query
    fn estimate_cost(&self, input_tokens: usize, output_tokens: usize) -> f64;
}

/// Mock dispatcher for testing
pub struct MockSubLlmDispatcher {
    model_id: String,
    cost_per_1k_input: f64,
    cost_per_1k_output: f64,
}

impl MockSubLlmDispatcher {
    pub fn new(model_id: &str) -> Self {
        Self {
            model_id: model_id.to_string(),
            cost_per_1k_input: 0.001,
            cost_per_1k_output: 0.002,
        }
    }
}

#[async_trait]
impl SubLlmDispatcher for MockSubLlmDispatcher {
    async fn dispatch(&self, prompt: &str, context: &str, depth: usize) -> Result<String> {
        // Simulate processing delay proportional to context size
        let delay_ms = (context.len() / 1000).clamp(10, 100);
        tokio::time::sleep(Duration::from_millis(delay_ms as u64)).await;

        // Generate mock response based on prompt type
        let response = if prompt.contains("count") {
            format!("Count result: {}", context.lines().count())
        } else if prompt.contains("find") {
            format!("Found relevant content at depth {}", depth)
        } else if prompt.contains("summarize") {
            format!(
                "Summary of {} chars: [content summary at depth {}]",
                context.len(),
                depth
            )
        } else {
            format!(
                "Processed {} chars with prompt '{}' at depth {}",
                context.len(),
                &prompt[..prompt.len().min(50)],
                depth
            )
        };

        Ok(response)
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn estimate_cost(&self, input_tokens: usize, output_tokens: usize) -> f64 {
        (input_tokens as f64 / 1000.0) * self.cost_per_1k_input
            + (output_tokens as f64 / 1000.0) * self.cost_per_1k_output
    }
}

// =============================================================================
// RECURSIVE LANGUAGE MODEL
// =============================================================================

/// Configuration for Recursive Language Model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RlmConfig {
    /// Maximum recursion depth for sub-LLM calls
    pub max_recursion_depth: usize,
    /// Default chunk size for context variables
    pub default_chunk_size: usize,
    /// Maximum context size in characters
    pub max_context_size: usize,
    /// Timeout for operations
    pub timeout: Duration,
    /// Whether to use speculative sub-calling
    pub speculative_subcalls: bool,
    /// Batch size for parallel sub-LLM calls
    pub subcall_batch_size: usize,
    /// Whether to use Titans memory for context compression
    pub use_titans_memory: bool,
}

impl Default for RlmConfig {
    fn default() -> Self {
        Self {
            max_recursion_depth: 3,
            default_chunk_size: 200_000, // ~200K chars per chunk as recommended
            max_context_size: 100_000_000, // 100M chars (~10M tokens)
            timeout: Duration::from_secs(300),
            speculative_subcalls: true,
            subcall_batch_size: 5,
            use_titans_memory: true,
        }
    }
}

/// Recursive Language Model for infinite context processing
///
/// Based on "Recursive Language Models" (Zhang et al., 2025), this implementation
/// treats long prompts as part of an external environment rather than feeding them
/// directly into the neural network.
///
/// ## Key Features
///
/// - **Infinite Context**: Process inputs up to 100× beyond model context windows
/// - **Recursive Sub-Calls**: Programmatically construct sub-tasks and invoke recursively
/// - **REPL Environment**: Load context as variables, interact via code
/// - **Emergent Patterns**: Filtering, chunking, verification through sub-LLM calls
pub struct RecursiveLM {
    /// Configuration
    config: RlmConfig,
    /// REPL environment
    repl: Arc<ReplEnvironment>,
    /// Root LLM dispatcher
    root_dispatcher: Arc<dyn SubLlmDispatcher>,
    /// Sub-LLM dispatcher (can be different/smaller model)
    sub_dispatcher: Arc<dyn SubLlmDispatcher>,
    /// Session statistics
    stats: RlmStats,
}

/// Statistics for RLM operations
#[derive(Debug, Default)]
pub struct RlmStats {
    pub queries_processed: AtomicU64,
    pub total_input_chars: AtomicU64,
    pub total_output_chars: AtomicU64,
    pub root_llm_calls: AtomicU64,
    pub sub_llm_calls: AtomicU64,
    pub avg_recursion_depth: AtomicU64,
    pub total_cost_cents: AtomicU64,
}

impl RecursiveLM {
    /// Create a new Recursive Language Model
    pub fn new(
        config: RlmConfig,
        root_dispatcher: Arc<dyn SubLlmDispatcher>,
        sub_dispatcher: Arc<dyn SubLlmDispatcher>,
    ) -> Self {
        let repl = Arc::new(ReplEnvironment::new(
            sub_dispatcher.clone(),
            config.max_recursion_depth,
        ));

        Self {
            config,
            repl,
            root_dispatcher,
            sub_dispatcher,
            stats: RlmStats::default(),
        }
    }

    /// Load context for processing
    ///
    /// The context is stored as a variable in the REPL environment,
    /// allowing symbolic manipulation without neural processing.
    pub async fn load_context(&self, name: &str, content: String) -> Result<ContextMetadata> {
        if content.len() > self.config.max_context_size {
            return Err(RlmError::ContextTooLarge {
                size: content.len(),
            });
        }

        self.stats
            .total_input_chars
            .fetch_add(content.len() as u64, Ordering::Relaxed);

        self.repl
            .load_context(name, content, self.config.default_chunk_size)
            .await
    }

    /// Query the RLM with automatic context decomposition
    ///
    /// The RLM will:
    /// 1. Analyze the query to determine strategy
    /// 2. Use REPL operations to examine context
    /// 3. Recursively sub-call LLMs on relevant chunks
    /// 4. Aggregate results into final answer
    pub async fn query(&self, query: &str) -> Result<RlmResponse> {
        let start = Instant::now();
        self.stats.queries_processed.fetch_add(1, Ordering::Relaxed);

        // Build context summary for root LLM
        let context_info = self.build_context_summary();

        // Root LLM determines strategy
        let strategy = self.determine_strategy(query, &context_info).await?;

        // Execute strategy
        let result = self.execute_strategy(query, strategy).await?;

        self.stats.root_llm_calls.fetch_add(1, Ordering::Relaxed);
        self.stats
            .total_output_chars
            .fetch_add(result.answer.len() as u64, Ordering::Relaxed);

        // Calculate cost before consuming result
        let total_cost = self.estimate_cost(&result);

        Ok(RlmResponse {
            answer: result.answer,
            trajectory: result.trajectory,
            stats: RlmResponseStats {
                duration: start.elapsed(),
                root_calls: 1,
                sub_calls: self.repl.get_stats().sub_llm_calls as usize,
                chunks_processed: self.repl.get_stats().chunks_accessed as usize,
                total_cost,
            },
        })
    }

    /// Build summary of loaded context for root LLM
    fn build_context_summary(&self) -> String {
        let vars = self.repl.list_variables();
        let mut summary = String::from("Available context variables:\n");

        for var_name in vars {
            if let Some(info) = self.repl.get_variable_info(&var_name) {
                summary.push_str(&format!(
                    "- {}: {} total chars, {} chunks (sizes: {:?})\n",
                    info.name,
                    info.total_chars,
                    info.num_chunks,
                    &info.chunk_sizes[..info.chunk_sizes.len().min(5)]
                ));
            }
        }

        summary
    }

    /// Determine processing strategy based on query and context
    async fn determine_strategy(&self, query: &str, _context_info: &str) -> Result<QueryStrategy> {
        // Analyze query type
        let query_lower = query.to_lowercase();

        let strategy = if query_lower.contains("find")
            || query_lower.contains("search")
            || query_lower.contains("where")
        {
            // Search-type query: use regex/keyword filtering
            QueryStrategy::FilterAndSearch {
                keywords: extract_keywords(query),
                use_regex: query.contains("*") || query.contains("\\"),
            }
        } else if query_lower.contains("count")
            || query_lower.contains("how many")
            || query_lower.contains("list all")
        {
            // Aggregation query: process all chunks
            QueryStrategy::ChunkAndAggregate {
                chunk_batch_size: self.config.subcall_batch_size,
            }
        } else if query_lower.contains("summarize") || query_lower.contains("overview") {
            // Summary query: hierarchical summarization
            QueryStrategy::HierarchicalSummarize { levels: 2 }
        } else {
            // Default: adaptive strategy
            QueryStrategy::Adaptive
        };

        Ok(strategy)
    }

    /// Execute the determined strategy
    async fn execute_strategy(
        &self,
        query: &str,
        strategy: QueryStrategy,
    ) -> Result<StrategyResult> {
        let mut trajectory = Vec::new();

        let answer = match strategy {
            QueryStrategy::FilterAndSearch {
                keywords,
                use_regex,
            } => {
                self.execute_filter_search(query, &keywords, use_regex, &mut trajectory)
                    .await?
            }
            QueryStrategy::ChunkAndAggregate { chunk_batch_size } => {
                self.execute_chunk_aggregate(query, chunk_batch_size, &mut trajectory)
                    .await?
            }
            QueryStrategy::HierarchicalSummarize { levels } => {
                self.execute_hierarchical_summarize(query, levels, &mut trajectory)
                    .await?
            }
            QueryStrategy::Adaptive => self.execute_adaptive(query, &mut trajectory).await?,
        };

        Ok(StrategyResult { answer, trajectory })
    }

    /// Execute filter and search strategy
    async fn execute_filter_search(
        &self,
        query: &str,
        keywords: &[String],
        use_regex: bool,
        trajectory: &mut Vec<TrajectoryStep>,
    ) -> Result<String> {
        let vars = self.repl.list_variables();
        let mut relevant_chunks = Vec::new();

        for var_name in vars {
            for keyword in keywords {
                let matches = if use_regex {
                    self.repl.search_regex(&var_name, keyword).await?
                } else {
                    self.repl.search_keyword(&var_name, keyword).await?
                };

                trajectory.push(TrajectoryStep {
                    action: format!(
                        "Search '{}' in {} ({} matches)",
                        keyword,
                        var_name,
                        matches.len()
                    ),
                    result: format!("Found {} chunks", matches.len()),
                });

                relevant_chunks.extend(matches);
            }
        }

        // Process relevant chunks with sub-LLM calls
        let mut results = Vec::new();
        for (chunk_idx, chunk_content) in relevant_chunks.into_iter().take(10) {
            let result = self
                .repl
                .sub_llm_call(
                    &format!("Given this context, answer: {}", query),
                    &chunk_content,
                )
                .await?;

            trajectory.push(TrajectoryStep {
                action: format!("Sub-LLM call on chunk {}", chunk_idx),
                result: result.clone(),
            });

            results.push(result);
        }

        // Aggregate results
        let aggregated = if results.is_empty() {
            "No relevant content found for the query.".to_string()
        } else {
            results.join("\n\n")
        };

        Ok(aggregated)
    }

    /// Execute chunk and aggregate strategy
    async fn execute_chunk_aggregate(
        &self,
        query: &str,
        batch_size: usize,
        trajectory: &mut Vec<TrajectoryStep>,
    ) -> Result<String> {
        let vars = self.repl.list_variables();
        let mut all_results = Vec::new();

        for var_name in vars {
            if let Some(info) = self.repl.get_variable_info(&var_name) {
                // Process chunks in batches
                for batch_start in (0..info.num_chunks).step_by(batch_size) {
                    let batch_end = (batch_start + batch_size).min(info.num_chunks);
                    let mut batch_results = Vec::new();

                    for chunk_idx in batch_start..batch_end {
                        let chunk = self.repl.get_chunk(&var_name, chunk_idx).await?;
                        let result = self
                            .repl
                            .sub_llm_call(
                                &format!("Process this chunk for query: {}", query),
                                &chunk,
                            )
                            .await?;

                        batch_results.push(result);
                    }

                    trajectory.push(TrajectoryStep {
                        action: format!(
                            "Process batch {}-{} of {}",
                            batch_start, batch_end, var_name
                        ),
                        result: format!("{} results", batch_results.len()),
                    });

                    all_results.extend(batch_results);
                }
            }
        }

        // Final aggregation
        let final_prompt = format!(
            "Aggregate these results to answer: {}\n\nResults:\n{}",
            query,
            all_results.join("\n---\n")
        );

        let final_answer = self.root_dispatcher.dispatch(&final_prompt, "", 0).await?;

        trajectory.push(TrajectoryStep {
            action: "Final aggregation".to_string(),
            result: final_answer.clone(),
        });

        Ok(final_answer)
    }

    /// Execute hierarchical summarization strategy
    async fn execute_hierarchical_summarize(
        &self,
        query: &str,
        levels: usize,
        trajectory: &mut Vec<TrajectoryStep>,
    ) -> Result<String> {
        let vars = self.repl.list_variables();
        let mut current_summaries = Vec::new();

        // Level 0: Summarize individual chunks
        for var_name in vars {
            if let Some(info) = self.repl.get_variable_info(&var_name) {
                for chunk_idx in 0..info.num_chunks {
                    let chunk = self.repl.get_chunk(&var_name, chunk_idx).await?;
                    let summary = self
                        .repl
                        .sub_llm_call("Summarize this content briefly:", &chunk)
                        .await?;
                    current_summaries.push(summary);
                }
            }
        }

        trajectory.push(TrajectoryStep {
            action: "Level 0: Chunk summaries".to_string(),
            result: format!("{} summaries", current_summaries.len()),
        });

        // Higher levels: Aggregate summaries
        for level in 1..=levels {
            if current_summaries.len() <= 1 {
                break;
            }

            let mut next_level_summaries = Vec::new();
            let group_size = 5;

            for group in current_summaries.chunks(group_size) {
                let combined = group.join("\n\n");
                let summary = self
                    .repl
                    .sub_llm_call("Summarize these summaries:", &combined)
                    .await?;
                next_level_summaries.push(summary);
            }

            trajectory.push(TrajectoryStep {
                action: format!("Level {}: Aggregate summaries", level),
                result: format!("{} summaries", next_level_summaries.len()),
            });

            current_summaries = next_level_summaries;
        }

        // Final answer generation
        let context = current_summaries.join("\n\n");
        let final_answer = self
            .root_dispatcher
            .dispatch(
                &format!("Based on these summaries, answer: {}", query),
                &context,
                0,
            )
            .await?;

        Ok(final_answer)
    }

    /// Execute adaptive strategy
    async fn execute_adaptive(
        &self,
        query: &str,
        trajectory: &mut Vec<TrajectoryStep>,
    ) -> Result<String> {
        // Start with keyword extraction and search
        let keywords = extract_keywords(query);

        trajectory.push(TrajectoryStep {
            action: "Adaptive: Extract keywords".to_string(),
            result: format!("Keywords: {:?}", keywords),
        });

        // Try filter search first
        let search_result = self
            .execute_filter_search(query, &keywords, false, trajectory)
            .await?;

        if search_result.contains("No relevant content") {
            // Fallback to chunk aggregate
            self.execute_chunk_aggregate(query, 5, trajectory).await
        } else {
            Ok(search_result)
        }
    }

    /// Estimate cost of a result
    fn estimate_cost(&self, result: &StrategyResult) -> f64 {
        let input_tokens = result.trajectory.iter().map(|t| t.action.len() / 4).sum();
        let output_tokens = result.answer.len() / 4;
        self.root_dispatcher
            .estimate_cost(input_tokens, output_tokens)
    }

    /// Get REPL environment reference
    pub fn repl(&self) -> &Arc<ReplEnvironment> {
        &self.repl
    }

    /// Get current statistics
    pub fn stats(&self) -> RlmStatsSnapshot {
        RlmStatsSnapshot {
            queries_processed: self.stats.queries_processed.load(Ordering::Relaxed),
            total_input_chars: self.stats.total_input_chars.load(Ordering::Relaxed),
            total_output_chars: self.stats.total_output_chars.load(Ordering::Relaxed),
            root_llm_calls: self.stats.root_llm_calls.load(Ordering::Relaxed),
            sub_llm_calls: self.stats.sub_llm_calls.load(Ordering::Relaxed),
            repl_stats: self.repl.get_stats(),
        }
    }
}

/// Query processing strategy
#[derive(Debug, Clone)]
enum QueryStrategy {
    /// Use regex/keyword filtering to find relevant chunks
    FilterAndSearch {
        keywords: Vec<String>,
        use_regex: bool,
    },
    /// Process all chunks and aggregate results
    ChunkAndAggregate { chunk_batch_size: usize },
    /// Hierarchical summarization
    HierarchicalSummarize { levels: usize },
    /// Adaptive strategy based on initial probing
    Adaptive,
}

/// Result from strategy execution
struct StrategyResult {
    answer: String,
    trajectory: Vec<TrajectoryStep>,
}

/// Step in the RLM trajectory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrajectoryStep {
    pub action: String,
    pub result: String,
}

/// Response from RLM query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RlmResponse {
    /// Final answer
    pub answer: String,
    /// Execution trajectory
    pub trajectory: Vec<TrajectoryStep>,
    /// Statistics
    pub stats: RlmResponseStats,
}

/// Statistics for a single RLM response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RlmResponseStats {
    pub duration: Duration,
    pub root_calls: usize,
    pub sub_calls: usize,
    pub chunks_processed: usize,
    pub total_cost: f64,
}

/// Snapshot of RLM statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RlmStatsSnapshot {
    pub queries_processed: u64,
    pub total_input_chars: u64,
    pub total_output_chars: u64,
    pub root_llm_calls: u64,
    pub sub_llm_calls: u64,
    pub repl_stats: ReplStatsSnapshot,
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Extract keywords from a query
fn extract_keywords(query: &str) -> Vec<String> {
    let stop_words = [
        "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
        "do", "does", "did", "will", "would", "could", "should", "may", "might", "must", "shall",
        "can", "need", "dare", "ought", "used", "to", "of", "in", "for", "on", "with", "at", "by",
        "from", "as", "into", "through", "during", "before", "after", "above", "below", "between",
        "under", "again", "further", "then", "once", "here", "there", "when", "where", "why",
        "how", "all", "each", "few", "more", "most", "other", "some", "such", "no", "nor", "not",
        "only", "own", "same", "so", "than", "too", "very", "just", "and", "but", "if", "or",
        "because", "until", "while", "what", "which", "who", "whom", "this", "that", "these",
        "those", "am", "find", "search", "count", "list", "show", "get",
    ];

    query
        .split_whitespace()
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() > 2 && !stop_words.contains(&w.as_str()))
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_variable_chunking() {
        let content = "Line 1\nLine 2\nLine 3\nLine 4\nLine 5\n".repeat(100);
        let var = ContextVariable::new("test", content.clone(), 100);

        assert!(var.num_chunks() > 1);
        assert_eq!(var.total_chars, content.len());

        // Verify all chunks concatenate back to original
        let mut reconstructed = String::new();
        for i in 0..var.num_chunks() {
            reconstructed.push_str(var.get_chunk(i).unwrap());
        }
        assert_eq!(reconstructed, content);
    }

    #[test]
    fn test_keyword_search() {
        let content = "Hello world\nFoo bar\nHello again\nBaz qux";
        let var = ContextVariable::new("test", content.to_string(), 20);

        let results = var.search_keyword("hello");
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_regex_search() {
        let content = "Email: test@example.com\nAnother email: foo@bar.org\nNo email here";
        let var = ContextVariable::new("test", content.to_string(), 30);

        let results = var.search_regex(r"[\w]+@[\w]+\.\w+").unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_extract_keywords() {
        let query = "Find all the references to quantum computing in the document";
        let keywords = extract_keywords(query);

        assert!(keywords.contains(&"references".to_string()));
        assert!(keywords.contains(&"quantum".to_string()));
        assert!(keywords.contains(&"computing".to_string()));
        assert!(keywords.contains(&"document".to_string()));
        assert!(!keywords.contains(&"the".to_string()));
        assert!(!keywords.contains(&"to".to_string()));
    }

    #[tokio::test]
    async fn test_repl_environment() {
        let dispatcher = Arc::new(MockSubLlmDispatcher::new("mock-model"));
        let repl = ReplEnvironment::new(dispatcher, 3);

        // Load context
        let content = "Test content ".repeat(1000);
        let meta = repl.load_context("doc", content, 1000).await.unwrap();

        assert_eq!(meta.name, "doc");
        assert!(meta.num_chunks > 1);

        // Get chunk
        let chunk = repl.get_chunk("doc", 0).await.unwrap();
        assert!(!chunk.is_empty());

        // Keyword search
        let results = repl.search_keyword("doc", "content").await.unwrap();
        assert!(!results.is_empty());
    }

    #[tokio::test]
    async fn test_recursive_lm() {
        let root = Arc::new(MockSubLlmDispatcher::new("root-model"));
        let sub = Arc::new(MockSubLlmDispatcher::new("sub-model"));

        let config = RlmConfig::default();
        let rlm = RecursiveLM::new(config, root, sub);

        // Load large context
        let content = "Important fact: The answer is 42.\n".repeat(10000);
        let meta = rlm.load_context("knowledge", content).await.unwrap();

        assert!(meta.total_chars > 100000);

        // Query
        let response = rlm.query("Find the answer").await.unwrap();
        assert!(!response.answer.is_empty());
        assert!(!response.trajectory.is_empty());
    }

    #[test]
    fn test_session_id() {
        let id1 = SessionId::new();
        let id2 = SessionId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_chunk_id() {
        let id = ChunkId::new(42);
        assert_eq!(id.0, 42);
    }

    #[test]
    fn test_get_lines() {
        let content = "Line 0\nLine 1\nLine 2\nLine 3\nLine 4";
        let var = ContextVariable::new("test", content.to_string(), 100);

        let lines = var.get_lines(1, 3);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "Line 1");
        assert_eq!(lines[1], "Line 2");
    }

    #[test]
    fn test_line_count() {
        let content = "Line 1\nLine 2\nLine 3";
        let var = ContextVariable::new("test", content.to_string(), 100);
        assert_eq!(var.line_count(), 3);
    }
}
