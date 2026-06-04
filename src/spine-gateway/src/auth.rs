//! Optional bearer-token authentication middleware.
//!
//! The gateway's OpenAPI schema declares a `bearer` security scheme;
//! this module is the actual enforcement. It's opt-in via an
//! environment variable so existing deployments keep working unchanged
//! while new deployments can require auth without changing the binary.
//!
//! ## Activation
//!
//! Set `SPINE_GATEWAY_BEARER_TOKEN` to a non-empty secret before
//! launching the gateway. With the variable set, every request to
//! every route (except `/health`, `/ready`, and the Swagger UI) must
//! carry `Authorization: Bearer <secret>`. Comparison is
//! constant-time via [`subtle::ConstantTimeEq`].
//!
//! With the variable unset, the middleware is a no-op and the gateway
//! behaves exactly as it did before this module existed — startup logs
//! a `WARN` line so the deployer notices.
//!
//! ## Why an env var rather than a config field
//!
//! Bearer tokens are credentials and credentials should not live in
//! config files that get checked into version control. The env-var
//! channel matches the standard 12-factor pattern and is what
//! Kubernetes / Docker / systemd already wire up for secrets.

use axum::body::Body;
use axum::extract::Request;
use axum::http::{header, Response, StatusCode};
use axum::middleware::Next;
use serde_json::json;
use std::sync::Arc;
use subtle::ConstantTimeEq;

/// Loaded once at startup; `None` means "auth disabled".
#[derive(Clone)]
pub struct BearerConfig {
    expected: Arc<Vec<u8>>,
}

impl BearerConfig {
    /// Read `SPINE_GATEWAY_BEARER_TOKEN` from the environment. Returns
    /// `None` (auth disabled) when the variable is unset or empty.
    pub fn from_env() -> Option<Self> {
        let raw = std::env::var("SPINE_GATEWAY_BEARER_TOKEN").ok()?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        Some(Self {
            expected: Arc::new(trimmed.as_bytes().to_vec()),
        })
    }

    /// Build a config from an explicit token. Test-only — production
    /// must read the secret from the environment via [`Self::from_env`]
    /// so it isn't baked into the binary.
    #[cfg(test)]
    pub fn from_token(token: &str) -> Self {
        Self {
            expected: Arc::new(token.as_bytes().to_vec()),
        }
    }

    /// True when `presented` matches the configured secret. Constant
    /// time over the byte length to avoid leaking the prefix length via
    /// timing.
    fn matches(&self, presented: &[u8]) -> bool {
        // ConstantTimeEq requires equal-length operands; pad/truncate
        // by failing fast on length mismatch then ct-comparing the
        // common prefix. Length leak is bounded to "wrong-length or
        // not" which is the same as the comparison being constant-time.
        if presented.len() != self.expected.len() {
            return false;
        }
        presented.ct_eq(&self.expected).into()
    }
}

/// Axum middleware. Skips auth on a small allowlist of unauthenticated
/// paths (health probes, OpenAPI docs) and otherwise requires
/// `Authorization: Bearer <token>`.
pub async fn require_bearer(
    cfg: BearerConfig,
    req: Request,
    next: Next,
) -> Response<Body> {
    // Routes that must remain reachable without credentials so
    // kube probes, load balancers, and humans browsing the docs
    // can still function. Everything else requires the bearer.
    let path = req.uri().path();
    if is_unauthenticated_path(path) {
        return next.run(req).await;
    }

    let header_value = match req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
    {
        Some(v) => v,
        None => return unauthorized("missing Authorization header"),
    };

    let presented = match header_value.strip_prefix("Bearer ") {
        Some(t) => t.trim(),
        None => return unauthorized("Authorization header must use `Bearer` scheme"),
    };

    if !cfg.matches(presented.as_bytes()) {
        return unauthorized("invalid bearer token");
    }

    next.run(req).await
}

fn is_unauthenticated_path(path: &str) -> bool {
    matches!(path, "/health" | "/ready")
        || path.starts_with("/swagger-ui")
        || path.starts_with("/api-docs")
}

fn unauthorized(reason: &str) -> Response<Body> {
    let body = serde_json::to_vec(&json!({ "error": "unauthorized", "reason": reason }))
        .expect("static json is serializable");
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::WWW_AUTHENTICATE, "Bearer realm=\"spine-gateway\"")
        .body(Body::from(body))
        .expect("response is well-formed")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// `cargo test` runs tests in parallel by default, so multiple
    /// tests that mutate the same env var race. Serialise just the
    /// env-touching cases through this mutex so one test sees a
    /// consistent view at a time. The non-env tests run in parallel.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn matches_constant_time_equal_strings() {
        let cfg = BearerConfig::from_token("hunter2");
        assert!(cfg.matches(b"hunter2"));
    }

    #[test]
    fn matches_rejects_wrong_token() {
        let cfg = BearerConfig::from_token("hunter2");
        assert!(!cfg.matches(b"wrong"));
        assert!(!cfg.matches(b""));
        assert!(!cfg.matches(b"hunter3"));
    }

    #[test]
    fn matches_rejects_length_mismatch() {
        // Different length must fail without comparing — but the
        // function must still return false, not panic.
        let cfg = BearerConfig::from_token("abc");
        assert!(!cfg.matches(b"abcd"));
        assert!(!cfg.matches(b"ab"));
    }

    #[test]
    fn from_env_returns_none_when_unset() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::remove_var("SPINE_GATEWAY_BEARER_TOKEN");
        assert!(BearerConfig::from_env().is_none());
    }

    #[test]
    fn from_env_returns_none_when_empty() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("SPINE_GATEWAY_BEARER_TOKEN", "");
        assert!(BearerConfig::from_env().is_none());
        std::env::remove_var("SPINE_GATEWAY_BEARER_TOKEN");
    }

    #[test]
    fn from_env_returns_some_when_set() {
        let _g = ENV_LOCK.lock().unwrap();
        std::env::set_var("SPINE_GATEWAY_BEARER_TOKEN", "abc");
        let cfg = BearerConfig::from_env().expect("env var was just set");
        assert!(cfg.matches(b"abc"));
        std::env::remove_var("SPINE_GATEWAY_BEARER_TOKEN");
    }

    #[test]
    fn unauthenticated_paths_skip_auth() {
        assert!(is_unauthenticated_path("/health"));
        assert!(is_unauthenticated_path("/ready"));
        assert!(is_unauthenticated_path("/swagger-ui/index.html"));
        assert!(is_unauthenticated_path("/api-docs/openapi.json"));
    }

    #[test]
    fn authenticated_paths_require_auth() {
        assert!(!is_unauthenticated_path("/api/sessions"));
        assert!(!is_unauthenticated_path("/v1/embeddings"));
        assert!(!is_unauthenticated_path("/v1/chat/completions"));
        assert!(!is_unauthenticated_path("/v1/agentic/capabilities"));
        assert!(!is_unauthenticated_path("/metrics"));
    }
}
