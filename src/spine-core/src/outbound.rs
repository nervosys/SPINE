//! Bounded HTTP navigation. Operator exceptions are exact origins, never URL prefixes.
use anyhow::{ensure, Result};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use spine_protocol::body::{
    ERR_OUTBOUND_ADDRESS_DENIED, ERR_OUTBOUND_BODY_FAILED, ERR_OUTBOUND_BUSY,
    ERR_OUTBOUND_CLIENT_FAILED, ERR_OUTBOUND_CREDENTIALS_DENIED, ERR_OUTBOUND_DISABLED,
    ERR_OUTBOUND_DNS_FAILED, ERR_OUTBOUND_DNS_LIMIT, ERR_OUTBOUND_ENCODING_DENIED,
    ERR_OUTBOUND_FRAGMENT_DENIED, ERR_OUTBOUND_HTTP_STATUS, ERR_OUTBOUND_INVALID_HOST,
    ERR_OUTBOUND_INVALID_PORT, ERR_OUTBOUND_INVALID_URL, ERR_OUTBOUND_INVALID_UTF8,
    ERR_OUTBOUND_REDIRECT_DENIED, ERR_OUTBOUND_REQUEST_FAILED, ERR_OUTBOUND_RESPONSE_LIMIT,
    ERR_OUTBOUND_SCHEME_DENIED, ERR_OUTBOUND_TIMEOUT, ERR_OUTBOUND_URL_LIMIT,
};
use std::{
    net::IpAddr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::Semaphore;

/// A failed fetch. `code` is one of the `ERR_OUTBOUND_*` codes of
/// `spine_protocol::body`, and the daemon sends it as the wire code without
/// inspecting any text. `status` is set only for `outbound_http_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FetchError {
    code: &'static str,
    status: Option<u16>,
}

impl FetchError {
    const fn new(code: &'static str) -> Self {
        Self { code, status: None }
    }

    /// The stable wire code.
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// The HTTP status, for `outbound_http_status` only.
    pub fn status(&self) -> Option<u16> {
        self.status
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.status {
            Some(status) => write!(f, "{}_{status}", self.code),
            None => f.write_str(self.code),
        }
    }
}

impl std::error::Error for FetchError {}

fn require(condition: bool, code: &'static str) -> Result<(), FetchError> {
    if condition {
        Ok(())
    } else {
        Err(FetchError::new(code))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FetchConfig {
    pub enabled: bool,
    pub internal_origins: Vec<String>,
    pub max_response_bytes: usize,
    pub timeout_secs: u64,
    pub max_concurrent: usize,
}

impl Default for FetchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            internal_origins: Vec::new(),
            max_response_bytes: 2 * 1024 * 1024,
            timeout_secs: 30,
            max_concurrent: 16,
        }
    }
}

fn parse_url(input: &str) -> Result<Url, FetchError> {
    require(input.len() <= 8192, ERR_OUTBOUND_URL_LIMIT)?;
    let url = Url::parse(input).map_err(|_| FetchError::new(ERR_OUTBOUND_INVALID_URL))?;
    require(
        matches!(url.scheme(), "http" | "https"),
        ERR_OUTBOUND_SCHEME_DENIED,
    )?;
    require(url.host_str().is_some(), ERR_OUTBOUND_INVALID_HOST)?;
    require(
        url.username().is_empty() && url.password().is_none(),
        ERR_OUTBOUND_CREDENTIALS_DENIED,
    )?;
    require(url.fragment().is_none(), ERR_OUTBOUND_FRAGMENT_DENIED)?;
    Ok(url)
}

impl FetchConfig {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=8 * 1024 * 1024).contains(&self.max_response_bytes),
            "invalid outbound response limit"
        );
        ensure!(
            (1..=300).contains(&self.timeout_secs),
            "invalid outbound timeout"
        );
        ensure!(
            (1..=256).contains(&self.max_concurrent),
            "invalid outbound concurrency"
        );
        ensure!(
            self.internal_origins.len() <= 64,
            "too many outbound internal origins"
        );
        for origin in &self.internal_origins {
            let url = parse_url(origin)?;
            ensure!(
                url.path() == "/" && url.query().is_none(),
                "outbound exception must be an origin"
            );
        }
        Ok(())
    }
}

/// Conservative public-unicast policy, also excluding documentation and transition ranges.
fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(a == 0
                || a == 10
                || a == 127
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && (b == 168 || (b == 0 && (c == 0 || c == 2))))
                || (a == 192 && b == 88 && c == 99)
                || (a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return public_address(IpAddr::V4(v4));
            }
            let s = ip.segments();
            (s[0] & 0xe000) == 0x2000
                && !(s[0] == 0x2001 && s[1] < 0x0200)
                && !(s[0] == 0x2001 && s[1] == 0x0db8)
                && s[0] != 0x2002
                && !(s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
}

#[derive(Default)]
struct FetchCounters {
    started: AtomicU64,
    succeeded: AtomicU64,
    failed: AtomicU64,
    cancelled: AtomicU64,
    denied: AtomicU64,
    busy: AtomicU64,
    timed_out: AtomicU64,
    response_limit_failures: AtomicU64,
    bytes_received: AtomicU64,
}

#[derive(Debug, serde::Serialize)]
pub struct FetchSnapshot {
    pub enabled: bool,
    pub active: usize,
    pub limit: usize,
    pub started: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub cancelled: u64,
    pub denied: u64,
    pub busy: u64,
    pub timed_out: u64,
    pub response_limit_failures: u64,
    pub bytes_received: u64,
}

struct CallOutcome {
    counters: Arc<FetchCounters>,
    completed: bool,
}
impl CallOutcome {
    fn complete(&mut self, result: &Result<String, FetchError>) {
        let c = &self.counters;
        if let Err(error) = result {
            c.failed.fetch_add(1, Ordering::Relaxed);
            match error.code() {
                ERR_OUTBOUND_BUSY => {
                    c.busy.fetch_add(1, Ordering::Relaxed);
                }
                ERR_OUTBOUND_TIMEOUT => {
                    c.timed_out.fetch_add(1, Ordering::Relaxed);
                }
                ERR_OUTBOUND_RESPONSE_LIMIT => {
                    c.response_limit_failures.fetch_add(1, Ordering::Relaxed);
                }
                ERR_OUTBOUND_DISABLED
                | ERR_OUTBOUND_ADDRESS_DENIED
                | ERR_OUTBOUND_SCHEME_DENIED
                | ERR_OUTBOUND_CREDENTIALS_DENIED
                | ERR_OUTBOUND_FRAGMENT_DENIED
                | ERR_OUTBOUND_REDIRECT_DENIED
                | ERR_OUTBOUND_ENCODING_DENIED => {
                    c.denied.fetch_add(1, Ordering::Relaxed);
                }
                _ => {}
            }
        } else {
            c.succeeded.fetch_add(1, Ordering::Relaxed);
        }
        self.completed = true;
    }
}
impl Drop for CallOutcome {
    fn drop(&mut self) {
        if !self.completed {
            self.counters.cancelled.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[derive(Clone)]
pub struct Fetcher {
    config: FetchConfig,
    exceptions: Vec<Url>,
    admission: Arc<Semaphore>,
    counters: Arc<FetchCounters>,
}

impl Fetcher {
    pub fn new(config: FetchConfig) -> Result<Self> {
        config.validate()?;
        let exceptions = config
            .internal_origins
            .iter()
            .map(|s| parse_url(s))
            .collect::<Result<_, FetchError>>()?;
        Ok(Self {
            admission: Arc::new(Semaphore::new(config.max_concurrent)),
            config,
            exceptions,
            counters: Arc::new(FetchCounters::default()),
        })
    }

    fn internal(&self, url: &Url) -> bool {
        self.exceptions
            .iter()
            .any(|allowed| allowed.origin() == url.origin())
    }

    pub fn snapshot(&self) -> FetchSnapshot {
        let c = &self.counters;
        FetchSnapshot {
            enabled: self.config.enabled,
            active: self
                .config
                .max_concurrent
                .saturating_sub(self.admission.available_permits()),
            limit: self.config.max_concurrent,
            started: c.started.load(Ordering::Relaxed),
            succeeded: c.succeeded.load(Ordering::Relaxed),
            failed: c.failed.load(Ordering::Relaxed),
            cancelled: c.cancelled.load(Ordering::Relaxed),
            denied: c.denied.load(Ordering::Relaxed),
            busy: c.busy.load(Ordering::Relaxed),
            timed_out: c.timed_out.load(Ordering::Relaxed),
            response_limit_failures: c.response_limit_failures.load(Ordering::Relaxed),
            bytes_received: c.bytes_received.load(Ordering::Relaxed),
        }
    }

    pub async fn fetch_text(&self, input: &str) -> Result<String, FetchError> {
        self.counters.started.fetch_add(1, Ordering::Relaxed);
        let mut outcome = CallOutcome {
            counters: self.counters.clone(),
            completed: false,
        };
        let result = self.fetch_checked(input).await;
        outcome.complete(&result);
        result
    }

    async fn fetch_checked(&self, input: &str) -> Result<String, FetchError> {
        require(self.config.enabled, ERR_OUTBOUND_DISABLED)?;
        let url = parse_url(input)?;
        tokio::time::timeout(
            Duration::from_secs(self.config.timeout_secs),
            self.fetch(url),
        )
        .await
        .map_err(|_| FetchError::new(ERR_OUTBOUND_TIMEOUT))?
    }

    async fn fetch(&self, url: Url) -> Result<String, FetchError> {
        let _lease = self
            .admission
            .try_acquire()
            .map_err(|_| FetchError::new(ERR_OUTBOUND_BUSY))?;
        // Resolve once, check the complete answer, then give exactly that answer to the connector.
        // A fresh client prevents a pooled connection from bypassing the current DNS decision.
        let host = url
            .host_str()
            .ok_or(FetchError::new(ERR_OUTBOUND_INVALID_HOST))?;
        let host = host.trim_start_matches('[').trim_end_matches(']');
        let port = url
            .port_or_known_default()
            .ok_or(FetchError::new(ERR_OUTBOUND_INVALID_PORT))?;
        let addresses: Vec<_> = tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| FetchError::new(ERR_OUTBOUND_DNS_FAILED))?
            .take(65)
            .collect();
        require(
            !addresses.is_empty() && addresses.len() <= 64,
            ERR_OUTBOUND_DNS_LIMIT,
        )?;
        require(
            self.internal(&url) || addresses.iter().all(|a| public_address(a.ip())),
            ERR_OUTBOUND_ADDRESS_DENIED,
        )?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .connect_timeout(Duration::from_secs(self.config.timeout_secs.min(10)))
            .timeout(Duration::from_secs(self.config.timeout_secs))
            .resolve_to_addrs(host, &addresses)
            .build()
            .map_err(|_| FetchError::new(ERR_OUTBOUND_CLIENT_FAILED))?;
        let mut response = client
            .get(url)
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    FetchError::new(ERR_OUTBOUND_TIMEOUT)
                } else {
                    FetchError::new(ERR_OUTBOUND_REQUEST_FAILED)
                }
            })?;
        require(
            !response.status().is_redirection(),
            ERR_OUTBOUND_REDIRECT_DENIED,
        )?;
        if !response.status().is_success() {
            return Err(FetchError {
                code: ERR_OUTBOUND_HTTP_STATUS,
                status: Some(response.status().as_u16()),
            });
        }
        require(
            response
                .headers()
                .get(reqwest::header::CONTENT_ENCODING)
                .is_none_or(|v| v == "identity"),
            ERR_OUTBOUND_ENCODING_DENIED,
        )?;
        require(
            response
                .content_length()
                .is_none_or(|size| size <= self.config.max_response_bytes as u64),
            ERR_OUTBOUND_RESPONSE_LIMIT,
        )?;
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| {
            if error.is_timeout() {
                FetchError::new(ERR_OUTBOUND_TIMEOUT)
            } else {
                FetchError::new(ERR_OUTBOUND_BODY_FAILED)
            }
        })? {
            self.counters
                .bytes_received
                .fetch_add(chunk.len() as u64, Ordering::Relaxed);
            require(
                chunk.len() <= self.config.max_response_bytes.saturating_sub(body.len()),
                ERR_OUTBOUND_RESPONSE_LIMIT,
            )?;
            body.extend_from_slice(&chunk);
        }
        String::from_utf8(body).map_err(|_| FetchError::new(ERR_OUTBOUND_INVALID_UTF8))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn address_policy_and_exact_origins() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "100.64.0.1",
            "169.254.169.254",
            "192.0.2.1",
            "198.18.0.1",
            "224.0.0.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "2001:db8::1",
            "2002::1",
        ] {
            assert!(!public_address(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700::1111"] {
            assert!(public_address(ip.parse().unwrap()));
        }
        let f = Fetcher::new(FetchConfig {
            internal_origins: vec!["http://localhost:8123".into()],
            ..Default::default()
        })
        .unwrap();
        assert!(f.internal(&parse_url("http://localhost:8123/page").unwrap()));
        assert!(!f.internal(&parse_url("http://localhost:8124/page").unwrap()));
        assert!(!f.internal(&parse_url("https://localhost:8123/page").unwrap()));
        assert!(!f.internal(&parse_url("http://localhost.example:8123/page").unwrap()));
    }

    #[test]
    fn url_and_status_failures_carry_their_own_codes() {
        let long = format!("http://example.com/{}", "a".repeat(8192));
        assert_eq!(parse_url(&long).unwrap_err().code(), ERR_OUTBOUND_URL_LIMIT);
        assert_eq!(
            parse_url("not a url").unwrap_err().code(),
            ERR_OUTBOUND_INVALID_URL
        );
        let status = FetchError {
            code: ERR_OUTBOUND_HTTP_STATUS,
            status: Some(503),
        };
        assert_eq!(status.status(), Some(503));
        assert_eq!(status.to_string(), "outbound_http_status_503");
        assert!(spine_protocol::body::ALL_ERROR_CODES.contains(&status.code()));
    }

    #[test]
    fn invalid_settings_and_urls_refused() {
        for input in [
            "file:///a",
            "http://user:secret@localhost/",
            "http://localhost/#a",
        ] {
            assert!(parse_url(input).is_err());
        }
        for origin in [
            "http://localhost/private",
            "http://localhost/?a=1",
            "ftp://localhost/",
        ] {
            assert!(FetchConfig {
                internal_origins: vec![origin.into()],
                ..Default::default()
            }
            .validate()
            .is_err());
        }
        assert!(FetchConfig {
            max_response_bytes: 0,
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(FetchConfig {
            timeout_secs: 0,
            ..Default::default()
        }
        .validate()
        .is_err());
        assert!(FetchConfig {
            max_concurrent: 0,
            ..Default::default()
        }
        .validate()
        .is_err());
    }

    async fn fixture(reply: &'static [u8]) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0; 4096];
            let _ = socket.read(&mut buf).await;
            let _ = socket.write_all(reply).await;
        });
        (origin, task)
    }

    #[tokio::test]
    async fn local_fetch_requires_exception_and_obeys_response_bounds() {
        let (origin, server) =
            fixture(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello")
                .await;
        let denied = Fetcher::new(FetchConfig::default())
            .unwrap()
            .fetch_text(&origin)
            .await
            .unwrap_err();
        assert_eq!(denied.to_string(), "outbound_address_denied");
        let f = Fetcher::new(FetchConfig {
            internal_origins: vec![origin.clone()],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(f.fetch_text(&origin).await.unwrap(), "hello");
        server.await.unwrap();
        for reply in [
            &b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello"[..],
            &b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n0\r\n\r\n"[..],
        ] {
            let (origin, server) = fixture(reply).await;
            let f = Fetcher::new(FetchConfig { internal_origins: vec![origin.clone()], max_response_bytes: 4, ..Default::default() }).unwrap();
            assert_eq!(f.fetch_text(&origin).await.unwrap_err().to_string(), "outbound_response_limit");
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn redirects_encoding_and_http_errors_are_not_consumed() {
        for (reply, error) in [
            (&b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/private\r\nContent-Length: 0\r\n\r\n"[..], "outbound_redirect_denied"),
            (&b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 0\r\n\r\n"[..], "outbound_encoding_denied"),
            (&b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\n\r\n"[..], "outbound_http_status_503"),
        ] {
            let (origin, server) = fixture(reply).await;
            let f = Fetcher::new(FetchConfig { internal_origins: vec![origin.clone()], ..Default::default() }).unwrap();
            let failure = f.fetch_text(&origin).await.unwrap_err();
            assert_eq!(failure.to_string(), error);
            if error.starts_with("outbound_http_status") {
                assert_eq!(failure.code(), ERR_OUTBOUND_HTTP_STATUS);
                assert_eq!(failure.status(), Some(503));
            }
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn cancellation_and_deadline_release_admission() {
        let f = Fetcher::new(FetchConfig {
            max_concurrent: 1,
            timeout_secs: 1,
            ..Default::default()
        })
        .unwrap();
        let held = f.admission.acquire().await.unwrap();
        assert_eq!(
            f.fetch_text("http://127.0.0.1/")
                .await
                .unwrap_err()
                .to_string(),
            "outbound_busy"
        );
        drop(held);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let f = Fetcher::new(FetchConfig {
            internal_origins: vec![origin.clone()],
            max_concurrent: 1,
            timeout_secs: 1,
            ..Default::default()
        })
        .unwrap();
        let worker = {
            let f = f.clone();
            let origin = origin.clone();
            tokio::spawn(async move { f.fetch_text(&origin).await })
        };
        let (_socket, _) = listener.accept().await.unwrap();
        assert_eq!(f.admission.available_permits(), 0);
        worker.abort();
        let _ = worker.await;
        assert_eq!(f.admission.available_permits(), 1);
        assert_eq!(f.snapshot().cancelled, 1);
        assert_eq!(
            f.fetch_text(&origin).await.unwrap_err().to_string(),
            "outbound_timeout"
        );
        assert_eq!(f.admission.available_permits(), 1);
        let snapshot = f.snapshot();
        assert_eq!(snapshot.started, 2);
        assert_eq!(snapshot.failed, 1);
        assert_eq!(snapshot.timed_out, 1);
        assert_eq!(snapshot.cancelled, 1);
    }

    #[tokio::test]
    async fn outcome_metrics_are_shared_and_do_not_contain_inputs() {
        let (origin, server) =
            fixture(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello")
                .await;
        let f = Fetcher::new(FetchConfig {
            internal_origins: vec![origin.clone()],
            max_concurrent: 1,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(f.clone().fetch_text(&origin).await.unwrap(), "hello");
        server.await.unwrap();
        assert!(f
            .fetch_text("http://127.0.0.1:1/private-marker")
            .await
            .is_err());
        let lease = f.admission.acquire().await.unwrap();
        assert!(f.fetch_text(&origin).await.is_err());
        drop(lease);
        let snapshot = f.snapshot();
        assert_eq!(snapshot.started, 3);
        assert_eq!(snapshot.succeeded, 1);
        assert_eq!(snapshot.failed, 2);
        assert_eq!(snapshot.denied, 1);
        assert_eq!(snapshot.busy, 1);
        assert_eq!(snapshot.bytes_received, 5);
        assert_eq!(snapshot.active, 0);
        let mut metrics = String::new();
        crate::operations::append_metrics(
            &mut metrics,
            crate::connections::ConnectionBudget::new(1, 1).snapshot(),
            snapshot,
        );
        assert!(metrics.contains("spine_outbound_calls_succeeded_total 1\n"));
        assert!(metrics.contains("spine_outbound_calls_denied_total 1\n"));
        assert!(
            !metrics.contains("127.0.0.1")
                && !metrics.contains("private-marker")
                && !metrics.contains(&origin)
        );
    }
}
