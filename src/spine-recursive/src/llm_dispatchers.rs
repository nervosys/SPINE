//! Real LLM Dispatcher implementations for production use
//!
//! This module provides actual LLM API integrations for the RLM system,
//! addressing the weakness of having only mock implementations.
//!
//! ## Dispatchers Provided
//!
//! - [`OpenAiDispatcher`]: OpenAI API (GPT-4, GPT-3.5-turbo)
//! - [`AnthropicDispatcher`]: Anthropic API (Claude 3)
//! - [`OfflineDispatcher`]: Fallback for offline operation
//! - [`AdaptiveDispatcher`]: Graceful degradation wrapper

use crate::{Result, RlmError, SubLlmDispatcher};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

// =============================================================================
// OPENAI API DISPATCHER
// =============================================================================

/// Configuration for OpenAI API
#[derive(Debug, Clone)]
pub struct OpenAiConfig {
    /// API key (from environment or explicit)
    pub api_key: String,
    /// Model to use (e.g., "gpt-4", "gpt-3.5-turbo")
    pub model: String,
    /// API base URL (for Azure or proxies)
    pub base_url: String,
    /// Maximum tokens to generate
    pub max_tokens: u32,
    /// Temperature for sampling
    pub temperature: f32,
    /// Request timeout
    pub timeout: Duration,
    /// Maximum retries on failure
    pub max_retries: u32,
    /// Cost per 1k input tokens (USD)
    pub cost_per_1k_input: f64,
    /// Cost per 1k output tokens (USD)
    pub cost_per_1k_output: f64,
}

impl Default for OpenAiConfig {
    fn default() -> Self {
        Self {
            api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            model: "gpt-4".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            max_tokens: 4096,
            temperature: 0.7,
            timeout: Duration::from_secs(60),
            max_retries: 3,
            cost_per_1k_input: 0.03, // GPT-4 pricing
            cost_per_1k_output: 0.06,
        }
    }
}

/// OpenAI API request format
#[derive(Debug, Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    max_tokens: u32,
    temperature: f32,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiMessage {
    role: String,
    content: String,
}

/// OpenAI API response format
#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

/// OpenAI-based sub-LLM dispatcher for production use
pub struct OpenAiDispatcher {
    config: OpenAiConfig,
    client: reqwest::Client,
    call_count: AtomicU64,
    total_tokens: AtomicU64,
}

impl OpenAiDispatcher {
    pub fn new(config: OpenAiConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            config,
            client,
            call_count: AtomicU64::new(0),
            total_tokens: AtomicU64::new(0),
        }
    }

    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("OPENAI_API_KEY").map_err(|_| RlmError::ModelError {
            message: "OPENAI_API_KEY environment variable not set".to_string(),
        })?;

        Ok(Self::new(OpenAiConfig {
            api_key,
            ..Default::default()
        }))
    }

    /// Create dispatcher for GPT-3.5-turbo (cheaper, faster)
    pub fn gpt35_turbo(api_key: String) -> Self {
        Self::new(OpenAiConfig {
            api_key,
            model: "gpt-3.5-turbo".to_string(),
            cost_per_1k_input: 0.0005,
            cost_per_1k_output: 0.0015,
            ..Default::default()
        })
    }

    /// Create dispatcher for GPT-4-turbo (balanced)
    pub fn gpt4_turbo(api_key: String) -> Self {
        Self::new(OpenAiConfig {
            api_key,
            model: "gpt-4-turbo".to_string(),
            cost_per_1k_input: 0.01,
            cost_per_1k_output: 0.03,
            ..Default::default()
        })
    }

    pub fn call_count(&self) -> u64 {
        self.call_count.load(Ordering::Relaxed)
    }

    pub fn total_tokens(&self) -> u64 {
        self.total_tokens.load(Ordering::Relaxed)
    }

    async fn call_api(&self, prompt: &str, context: &str) -> Result<(String, u32)> {
        // Combine prompt and context into a single user message
        let full_content = if context.is_empty() {
            prompt.to_string()
        } else {
            format!("Context:\n{}\n\nQuestion/Task:\n{}", context, prompt)
        };

        let request = OpenAiRequest {
            model: self.config.model.clone(),
            messages: vec![OpenAiMessage {
                role: "user".to_string(),
                content: full_content,
            }],
            max_tokens: self.config.max_tokens,
            temperature: self.config.temperature,
        };

        let mut last_error = None;

        for attempt in 0..self.config.max_retries {
            let response = self
                .client
                .post(format!("{}/chat/completions", self.config.base_url))
                .header("Authorization", format!("Bearer {}", self.config.api_key))
                .header("Content-Type", "application/json")
                .json(&request)
                .send()
                .await;

            match response {
                Ok(resp) if resp.status().is_success() => {
                    let body: OpenAiResponse =
                        resp.json().await.map_err(|e| RlmError::ModelError {
                            message: format!("Failed to parse response: {}", e),
                        })?;

                    let tokens = body.usage.map(|u| u.total_tokens).unwrap_or(0);
                    let content = body
                        .choices
                        .first()
                        .map(|c| c.message.content.clone())
                        .unwrap_or_default();

                    return Ok((content, tokens));
                }
                Ok(resp) => {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    last_error = Some(format!("API error {}: {}", status, body));

                    // Don't retry on 4xx errors (except rate limits)
                    if status.as_u16() >= 400 && status.as_u16() < 500 && status.as_u16() != 429 {
                        break;
                    }
                }
                Err(e) => {
                    last_error = Some(format!("Request failed: {}", e));
                }
            }

            // Exponential backoff
            if attempt < self.config.max_retries - 1 {
                tokio::time::sleep(Duration::from_millis(100 * 2_u64.pow(attempt))).await;
            }
        }

        Err(RlmError::SubLlmCallFailed {
            reason: last_error.unwrap_or_else(|| "Unknown error".to_string()),
        })
    }
}

#[async_trait]
impl SubLlmDispatcher for OpenAiDispatcher {
    async fn dispatch(&self, prompt: &str, context: &str, _depth: usize) -> Result<String> {
        let (content, tokens) = self.call_api(prompt, context).await?;

        self.call_count.fetch_add(1, Ordering::Relaxed);
        self.total_tokens
            .fetch_add(tokens as u64, Ordering::Relaxed);

        Ok(content)
    }

    fn model_id(&self) -> &str {
        &self.config.model
    }

    fn estimate_cost(&self, input_tokens: usize, output_tokens: usize) -> f64 {
        (input_tokens as f64 / 1000.0) * self.config.cost_per_1k_input
            + (output_tokens as f64 / 1000.0) * self.config.cost_per_1k_output
    }
}

// =============================================================================
// ANTHROPIC API DISPATCHER
// =============================================================================

/// Configuration for Anthropic API
#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    pub api_key: String,
    pub model: String,
    pub max_tokens: u32,
    pub timeout: Duration,
    pub cost_per_1k_input: f64,
    pub cost_per_1k_output: f64,
}

impl Default for AnthropicConfig {
    fn default() -> Self {
        Self {
            api_key: std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
            model: "claude-3-opus-20240229".to_string(),
            max_tokens: 4096,
            timeout: Duration::from_secs(60),
            cost_per_1k_input: 0.015, // Claude 3 Opus pricing
            cost_per_1k_output: 0.075,
        }
    }
}

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    messages: Vec<AnthropicMessage>,
    max_tokens: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct AnthropicMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContent>,
    usage: AnthropicUsage,
}

#[derive(Debug, Deserialize)]
struct AnthropicContent {
    text: String,
}

#[derive(Debug, Deserialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
}

/// Anthropic Claude-based sub-LLM dispatcher
pub struct AnthropicDispatcher {
    config: AnthropicConfig,
    client: reqwest::Client,
    call_count: AtomicU64,
    total_tokens: AtomicU64,
}

impl AnthropicDispatcher {
    pub fn new(config: AnthropicConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            config,
            client,
            call_count: AtomicU64::new(0),
            total_tokens: AtomicU64::new(0),
        }
    }

    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| RlmError::ModelError {
            message: "ANTHROPIC_API_KEY environment variable not set".to_string(),
        })?;

        Ok(Self::new(AnthropicConfig {
            api_key,
            ..Default::default()
        }))
    }

    /// Create dispatcher for Claude 3 Sonnet (balanced)
    pub fn claude3_sonnet(api_key: String) -> Self {
        Self::new(AnthropicConfig {
            api_key,
            model: "claude-3-sonnet-20240229".to_string(),
            cost_per_1k_input: 0.003,
            cost_per_1k_output: 0.015,
            ..Default::default()
        })
    }

    /// Create dispatcher for Claude 3 Haiku (fast, cheap)
    pub fn claude3_haiku(api_key: String) -> Self {
        Self::new(AnthropicConfig {
            api_key,
            model: "claude-3-haiku-20240307".to_string(),
            cost_per_1k_input: 0.00025,
            cost_per_1k_output: 0.00125,
            ..Default::default()
        })
    }

    pub fn call_count(&self) -> u64 {
        self.call_count.load(Ordering::Relaxed)
    }

    pub fn total_tokens(&self) -> u64 {
        self.total_tokens.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl SubLlmDispatcher for AnthropicDispatcher {
    async fn dispatch(&self, prompt: &str, context: &str, _depth: usize) -> Result<String> {
        let full_content = if context.is_empty() {
            prompt.to_string()
        } else {
            format!("Context:\n{}\n\nQuestion/Task:\n{}", context, prompt)
        };

        let request = AnthropicRequest {
            model: self.config.model.clone(),
            messages: vec![AnthropicMessage {
                role: "user".to_string(),
                content: full_content,
            }],
            max_tokens: self.config.max_tokens,
        };

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| RlmError::SubLlmCallFailed {
                reason: e.to_string(),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(RlmError::SubLlmCallFailed {
                reason: format!("Anthropic API error {}: {}", status, body),
            });
        }

        let body: AnthropicResponse = response.json().await.map_err(|e| RlmError::ModelError {
            message: format!("Failed to parse response: {}", e),
        })?;

        self.call_count.fetch_add(1, Ordering::Relaxed);
        let tokens = body.usage.input_tokens + body.usage.output_tokens;
        self.total_tokens
            .fetch_add(tokens as u64, Ordering::Relaxed);

        let content = body
            .content
            .first()
            .map(|c| c.text.clone())
            .unwrap_or_default();

        Ok(content)
    }

    fn model_id(&self) -> &str {
        &self.config.model
    }

    fn estimate_cost(&self, input_tokens: usize, output_tokens: usize) -> f64 {
        (input_tokens as f64 / 1000.0) * self.config.cost_per_1k_input
            + (output_tokens as f64 / 1000.0) * self.config.cost_per_1k_output
    }
}

// =============================================================================
// OFFLINE / LOCAL DISPATCHER
// =============================================================================

/// Offline dispatcher that works without network access
/// Uses simple heuristics for basic queries (addressing W5: offline mode)
pub struct OfflineDispatcher {
    model_id: String,
}

impl OfflineDispatcher {
    pub fn new() -> Self {
        Self {
            model_id: "offline-fallback".to_string(),
        }
    }
}

impl Default for OfflineDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SubLlmDispatcher for OfflineDispatcher {
    async fn dispatch(&self, prompt: &str, context: &str, depth: usize) -> Result<String> {
        // Offline mode: return the context as-is with minimal processing
        let response = if prompt.contains("count") {
            format!("[Offline] Line count: {}", context.lines().count())
        } else if prompt.contains("find") || prompt.contains("search") {
            // Basic keyword extraction from prompt
            let keywords: Vec<&str> = prompt
                .split_whitespace()
                .filter(|w| w.len() > 3)
                .take(5)
                .collect();
            let matches: Vec<&str> = context
                .lines()
                .filter(|line| {
                    keywords
                        .iter()
                        .any(|kw| line.to_lowercase().contains(&kw.to_lowercase()))
                })
                .take(10)
                .collect();
            format!(
                "[Offline] Found {} matching lines:\n{}",
                matches.len(),
                matches.join("\n")
            )
        } else if prompt.contains("summarize") {
            // Return first/last lines as "summary"
            let lines: Vec<&str> = context.lines().collect();
            let preview = if lines.len() > 10 {
                format!(
                    "{}\n...\n{}",
                    lines[..5].join("\n"),
                    lines[lines.len() - 5..].join("\n")
                )
            } else {
                context.to_string()
            };
            format!(
                "[Offline Summary] {} lines, {} chars. Preview:\n{}",
                lines.len(),
                context.len(),
                preview
            )
        } else {
            format!(
                "[Offline Mode - depth {}] Context preserved ({} chars). Query: {}",
                depth,
                context.len(),
                &prompt[..prompt.len().min(100)]
            )
        };

        Ok(response)
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn estimate_cost(&self, _input_tokens: usize, _output_tokens: usize) -> f64 {
        0.0 // Offline is free
    }
}

// =============================================================================
// ADAPTIVE DISPATCHER (GRACEFUL DEGRADATION)
// =============================================================================

/// Dispatcher that automatically falls back to offline mode on failure
/// Addresses W5: graceful degradation
pub struct AdaptiveDispatcher {
    primary: Arc<dyn SubLlmDispatcher>,
    fallback: OfflineDispatcher,
    consecutive_failures: AtomicU64,
    max_failures_before_fallback: u64,
    is_degraded: std::sync::atomic::AtomicBool,
}

impl AdaptiveDispatcher {
    pub fn new(primary: Arc<dyn SubLlmDispatcher>) -> Self {
        Self {
            primary,
            fallback: OfflineDispatcher::new(),
            consecutive_failures: AtomicU64::new(0),
            max_failures_before_fallback: 3,
            is_degraded: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn with_threshold(primary: Arc<dyn SubLlmDispatcher>, threshold: u64) -> Self {
        Self {
            primary,
            fallback: OfflineDispatcher::new(),
            consecutive_failures: AtomicU64::new(0),
            max_failures_before_fallback: threshold,
            is_degraded: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn is_degraded(&self) -> bool {
        self.is_degraded.load(Ordering::Relaxed)
    }

    /// Reset degraded state to retry primary
    pub fn reset(&self) {
        self.consecutive_failures.store(0, Ordering::Relaxed);
        self.is_degraded.store(false, Ordering::Relaxed);
    }
}

#[async_trait]
impl SubLlmDispatcher for AdaptiveDispatcher {
    async fn dispatch(&self, prompt: &str, context: &str, depth: usize) -> Result<String> {
        // If degraded, use fallback directly
        if self.is_degraded() {
            return self.fallback.dispatch(prompt, context, depth).await;
        }

        match self.primary.dispatch(prompt, context, depth).await {
            Ok(response) => {
                self.consecutive_failures.store(0, Ordering::Relaxed);
                Ok(response)
            }
            Err(e) => {
                let failures = self.consecutive_failures.fetch_add(1, Ordering::Relaxed) + 1;

                if failures >= self.max_failures_before_fallback {
                    self.is_degraded.store(true, Ordering::Relaxed);
                    // Return fallback response with error info
                    let fallback_response = self.fallback.dispatch(prompt, context, depth).await?;
                    Ok(format!("[Degraded due to: {}]\n{}", e, fallback_response))
                } else {
                    Err(e)
                }
            }
        }
    }

    fn model_id(&self) -> &str {
        if self.is_degraded() {
            self.fallback.model_id()
        } else {
            self.primary.model_id()
        }
    }

    fn estimate_cost(&self, input_tokens: usize, output_tokens: usize) -> f64 {
        if self.is_degraded() {
            0.0
        } else {
            self.primary.estimate_cost(input_tokens, output_tokens)
        }
    }
}

// =============================================================================
// LOAD-BALANCED DISPATCHER
// =============================================================================

/// Dispatcher that load-balances across multiple providers
/// Useful for high-throughput scenarios or cost optimization
pub struct LoadBalancedDispatcher {
    dispatchers: Vec<Arc<dyn SubLlmDispatcher>>,
    current_index: AtomicU64,
    strategy: LoadBalanceStrategy,
}

#[derive(Debug, Clone, Copy)]
pub enum LoadBalanceStrategy {
    RoundRobin,
    LeastCost,
}

impl LoadBalancedDispatcher {
    pub fn new(dispatchers: Vec<Arc<dyn SubLlmDispatcher>>, strategy: LoadBalanceStrategy) -> Self {
        Self {
            dispatchers,
            current_index: AtomicU64::new(0),
            strategy,
        }
    }

    pub fn round_robin(dispatchers: Vec<Arc<dyn SubLlmDispatcher>>) -> Self {
        Self::new(dispatchers, LoadBalanceStrategy::RoundRobin)
    }

    fn select_dispatcher(
        &self,
        input_tokens: usize,
        output_tokens: usize,
    ) -> &Arc<dyn SubLlmDispatcher> {
        match self.strategy {
            LoadBalanceStrategy::RoundRobin => {
                let idx = self.current_index.fetch_add(1, Ordering::Relaxed) as usize;
                &self.dispatchers[idx % self.dispatchers.len()]
            }
            LoadBalanceStrategy::LeastCost => self
                .dispatchers
                .iter()
                .min_by(|a, b| {
                    let cost_a = a.estimate_cost(input_tokens, output_tokens);
                    let cost_b = b.estimate_cost(input_tokens, output_tokens);
                    cost_a
                        .partial_cmp(&cost_b)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .unwrap_or(&self.dispatchers[0]),
        }
    }
}

#[async_trait]
impl SubLlmDispatcher for LoadBalancedDispatcher {
    async fn dispatch(&self, prompt: &str, context: &str, depth: usize) -> Result<String> {
        // Estimate tokens (rough: 1 token ≈ 4 chars)
        let input_tokens = (prompt.len() + context.len()) / 4;
        let output_tokens = 1000; // Estimate

        let dispatcher = self.select_dispatcher(input_tokens, output_tokens);
        dispatcher.dispatch(prompt, context, depth).await
    }

    fn model_id(&self) -> &str {
        "load-balanced"
    }

    fn estimate_cost(&self, input_tokens: usize, output_tokens: usize) -> f64 {
        // Return average cost across all dispatchers
        let total: f64 = self
            .dispatchers
            .iter()
            .map(|d| d.estimate_cost(input_tokens, output_tokens))
            .sum();
        total / self.dispatchers.len() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_offline_dispatcher() {
        let dispatcher = OfflineDispatcher::new();

        // Test count
        let response = dispatcher
            .dispatch("count lines", "line1\nline2\nline3", 0)
            .await
            .unwrap();
        assert!(response.contains("3"));

        // Test search
        let response = dispatcher
            .dispatch("find hello", "hello world\ngoodbye", 0)
            .await
            .unwrap();
        assert!(response.contains("hello"));

        // Test cost
        assert_eq!(dispatcher.estimate_cost(1000, 1000), 0.0);
    }

    #[tokio::test]
    async fn test_adaptive_fallback() {
        // Create a dispatcher that always fails
        struct FailingDispatcher;

        #[async_trait]
        impl SubLlmDispatcher for FailingDispatcher {
            async fn dispatch(
                &self,
                _prompt: &str,
                _context: &str,
                _depth: usize,
            ) -> Result<String> {
                Err(RlmError::SubLlmCallFailed {
                    reason: "Test failure".to_string(),
                })
            }
            fn model_id(&self) -> &str {
                "failing"
            }
            fn estimate_cost(&self, _: usize, _: usize) -> f64 {
                0.0
            }
        }

        let adaptive = AdaptiveDispatcher::new(Arc::new(FailingDispatcher));

        // First 3 calls should fail
        for _ in 0..3 {
            let _ = adaptive.dispatch("test", "context", 0).await;
        }

        // Now it should be degraded and use fallback
        assert!(adaptive.is_degraded());
        let response = adaptive.dispatch("test", "context", 0).await.unwrap();
        assert!(response.contains("Offline") || response.contains("Degraded"));
    }

    #[tokio::test]
    async fn test_load_balanced_round_robin() {
        struct CountingDispatcher {
            id: String,
            calls: AtomicU64,
        }

        #[async_trait]
        impl SubLlmDispatcher for CountingDispatcher {
            async fn dispatch(
                &self,
                _prompt: &str,
                _context: &str,
                _depth: usize,
            ) -> Result<String> {
                self.calls.fetch_add(1, Ordering::Relaxed);
                Ok(self.id.clone())
            }
            fn model_id(&self) -> &str {
                &self.id
            }
            fn estimate_cost(&self, _: usize, _: usize) -> f64 {
                0.0
            }
        }

        let d1 = Arc::new(CountingDispatcher {
            id: "d1".to_string(),
            calls: AtomicU64::new(0),
        });
        let d2 = Arc::new(CountingDispatcher {
            id: "d2".to_string(),
            calls: AtomicU64::new(0),
        });

        let lb = LoadBalancedDispatcher::round_robin(vec![
            d1.clone() as Arc<dyn SubLlmDispatcher>,
            d2.clone() as Arc<dyn SubLlmDispatcher>,
        ]);

        // Make 4 calls
        for _ in 0..4 {
            lb.dispatch("test", "", 0).await.unwrap();
        }

        // Each should have been called twice
        assert_eq!(d1.calls.load(Ordering::Relaxed), 2);
        assert_eq!(d2.calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_openai_config_default() {
        let config = OpenAiConfig::default();
        assert_eq!(config.model, "gpt-4");
        assert_eq!(config.max_tokens, 4096);
        assert!(config.cost_per_1k_input > 0.0);
    }

    #[test]
    fn test_anthropic_config_default() {
        let config = AnthropicConfig::default();
        assert!(config.model.contains("claude"));
        assert!(config.cost_per_1k_output > 0.0);
    }
}
