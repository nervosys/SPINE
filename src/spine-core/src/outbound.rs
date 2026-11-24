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
    collections::HashMap,
    net::IpAddr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, PoisonError,
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
    /// Ports a URL may name (or default to). Origins in `internal_origins` are exempt.
    pub allowed_ports: Vec<u16>,
    /// Longest wait for the next body chunk. The effective value is capped at `timeout_secs`.
    pub read_timeout_secs: u64,
    /// Most fetches one caller key may run at once. The effective value is capped at
    /// `max_concurrent`. Applies only to `fetch_text_for`.
    pub max_concurrent_per_caller: usize,
    /// Permit `internal_origins` whose host is a literal link-local or unspecified address.
    pub allow_link_local_exceptions: bool,
}

impl Default for FetchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            internal_origins: Vec::new(),
            max_response_bytes: 2 * 1024 * 1024,
            timeout_secs: 30,
            max_concurrent: 16,
            allowed_ports: vec![80, 443],
            read_timeout_secs: 10,
            max_concurrent_per_caller: 4,
            allow_link_local_exceptions: false,
        }
    }
}

/// A literal link-local (169.254.0.0/16, fe80::/10) or unspecified host, which
/// reaches cloud metadata services or the local machine.
fn link_local_or_unspecified_host(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let Ok(ip) = host.parse::<IpAddr>() else {
        return false;
    };
    match ip {
        IpAddr::V4(ip) => ip.is_link_local() || ip.is_unspecified(),
        IpAddr::V6(ip) => {
            ip.is_unspecified()
                || (ip.segments()[0] & 0xffc0) == 0xfe80
                || ip
                    .to_ipv4_mapped()
                    .is_some_and(|v4| v4.is_link_local() || v4.is_unspecified())
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
            (1..=300).contains(&self.read_timeout_secs),
            "invalid outbound read timeout"
        );
        ensure!(
            (1..=256).contains(&self.max_concurrent_per_caller),
            "invalid outbound per-caller concurrency"
        );
        ensure!(
            (1..=16).contains(&self.allowed_ports.len()) && !self.allowed_ports.contains(&0),
            "invalid outbound allowed ports"
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
            ensure!(
                self.allow_link_local_exceptions || !link_local_or_unspecified_host(&url),
                "outbound exception names a link-local or unspecified address"
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
                | ERR_OUTBOUND_INVALID_PORT
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

type CallerTable = Arc<Mutex<HashMap<String, usize>>>;

/// One running fetch of one caller. Dropping it (on success, error, timeout or
/// cancellation) decrements the caller's count and removes the entry at zero.
struct CallerSlot {
    table: CallerTable,
    key: String,
}
impl Drop for CallerSlot {
    fn drop(&mut self) {
        let mut table = self.table.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(count) = table.get_mut(&self.key) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                table.remove(&self.key);
            }
        }
    }
}

#[derive(Clone)]
pub struct Fetcher {
    config: FetchConfig,
    exceptions: Vec<Url>,
    admission: Arc<Semaphore>,
    counters: Arc<FetchCounters>,
    callers: CallerTable,
}

impl Fetcher {
    pub fn new(config: FetchConfig) -> Result<Self> {
        config.validate()?;
        let exceptions = config
            .internal_origins
            .iter()
            .map(|s| parse_url(s))
            .collect::<Result<Vec<Url>, FetchError>>()?;
        for exception in &exceptions {
            tracing::warn!(
                origin = %exception.origin().ascii_serialization(),
                "outbound internal-origin exception configured: it bypasses the address policy and port list"
            );
        }
        Ok(Self {
            admission: Arc::new(Semaphore::new(config.max_concurrent)),
            config,
            exceptions,
            counters: Arc::new(FetchCounters::default()),
            callers: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Refuses a port outside `allowed_ports` unless the origin is an exception.
    fn check_port(&self, url: &Url) -> Result<(), FetchError> {
        let port = url
            .port_or_known_default()
            .ok_or(FetchError::new(ERR_OUTBOUND_INVALID_PORT))?;
        require(
            self.config.allowed_ports.contains(&port) || self.internal(url),
            ERR_OUTBOUND_INVALID_PORT,
        )
    }

    fn admit_caller(&self, caller: Option<&str>) -> Result<Option<CallerSlot>, FetchError> {
        let Some(caller) = caller else {
            return Ok(None);
        };
        let cap = self
            .config
            .max_concurrent_per_caller
            .min(self.config.max_concurrent);
        let mut table = self.callers.lock().unwrap_or_else(PoisonError::into_inner);
        let count = table.entry(caller.to_owned()).or_insert(0);
        require(*count < cap, ERR_OUTBOUND_BUSY)?;
        *count += 1;
        Ok(Some(CallerSlot {
            table: self.callers.clone(),
            key: caller.to_owned(),
        }))
    }

    #[cfg(test)]
    fn callers_len(&self) -> usize {
        self.callers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
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
        self.fetch_counted(None, input).await
    }

    /// Like `fetch_text`, but also limits how many fetches one `caller` key (the
    /// daemon passes the session id) may run at once; over the cap is `outbound_busy`.
    pub async fn fetch_text_for(&self, caller: &str, input: &str) -> Result<String, FetchError> {
        self.fetch_counted(Some(caller), input).await
    }

    async fn fetch_counted(&self, caller: Option<&str>, input: &str) -> Result<String, FetchError> {
        self.counters.started.fetch_add(1, Ordering::Relaxed);
        let mut outcome = CallOutcome {
            counters: self.counters.clone(),
            completed: false,
        };
        let result = self.fetch_checked(caller, input).await;
        outcome.complete(&result);
        result
    }

    async fn fetch_checked(&self, caller: Option<&str>, input: &str) -> Result<String, FetchError> {
        require(self.config.enabled, ERR_OUTBOUND_DISABLED)?;
        let url = parse_url(input)?;
        // Before admission and before any DNS lookup.
        self.check_port(&url)?;
        tokio::time::timeout(
            Duration::from_secs(self.config.timeout_secs),
            self.fetch(url, caller),
        )
        .await
        .map_err(|_| FetchError::new(ERR_OUTBOUND_TIMEOUT))?
    }

    async fn fetch(&self, url: Url, caller: Option<&str>) -> Result<String, FetchError> {
        let _lease = self
            .admission
            .try_acquire()
            .map_err(|_| FetchError::new(ERR_OUTBOUND_BUSY))?;
        // Taken after the global permit, so the table never holds more entries than permits.
        let _slot = self.admit_caller(caller)?;
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
        let read_timeout =
            Duration::from_secs(self.config.read_timeout_secs.min(self.config.timeout_secs));
        while let Some(chunk) = tokio::time::timeout(read_timeout, response.chunk())
            .await
            .map_err(|_| FetchError::new(ERR_OUTBOUND_TIMEOUT))?
            .map_err(|error| {
                if error.is_timeout() {
                    FetchError::new(ERR_OUTBOUND_TIMEOUT)
                } else {
                    FetchError::new(ERR_OUTBOUND_BODY_FAILED)
                }
            })?
        {
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
        // With the listener's port allowed, the address policy is what refuses
        // loopback; with the default port list the port is refused first.
        let port: u16 = origin.rsplit(':').next().unwrap().parse().unwrap();
        let denied = Fetcher::new(FetchConfig {
            allowed_ports: vec![port],
            ..Default::default()
        })
        .unwrap()
        .fetch_text(&origin)
        .await
        .unwrap_err();
        assert_eq!(denied.to_string(), "outbound_address_denied");
        let refused = Fetcher::new(FetchConfig::default())
            .unwrap()
            .fetch_text(&origin)
            .await
            .unwrap_err();
        assert_eq!(refused.to_string(), "outbound_invalid_port");
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
            .fetch_text("http://127.0.0.1/private-marker")
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
        let metrics = crate::operations::render_metrics(&crate::operations::MetricsSnapshot {
            sessions_limit: 1,
            draining: false,
            connections: crate::connections::ConnectionBudget::new(1, 1).snapshot(),
            outbound: snapshot,
            execute_permits_in_use: 0,
            execute_permits_limit: 1,
            persistence_failing: false,
            resource_bytes: None,
            resource_limit_bytes: 0,
            records_held: 0,
            cache: crate::operations::CacheCounts::default(),
            mesh: None,
        });
        assert!(metrics.contains("spine_outbound_calls_succeeded_total 1\n"));
        assert!(metrics.contains("spine_outbound_calls_denied_total 1\n"));
        assert!(
            !metrics.contains("127.0.0.1")
                && !metrics.contains("private-marker")
                && !metrics.contains(&origin)
        );
    }

    async fn within<T>(future: impl std::future::Future<Output = T>) -> T {
        tokio::time::timeout(Duration::from_secs(20), future)
            .await
            .expect("test step exceeded its deadline")
    }

    /// Accepts one connection, reads the request, writes `head` and then stays silent.
    async fn stalling_origin(head: &'static [u8]) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0; 4096];
            let _ = socket.read(&mut buf).await;
            let _ = socket.write_all(head).await;
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        (origin, task)
    }

    #[tokio::test]
    async fn port_policy_refuses_before_dns_and_exempts_exceptions() {
        let f = Fetcher::new(FetchConfig::default()).unwrap();
        // These names would fail DNS or connect; the port check answers first.
        for url in [
            "http://example.com:8080/",
            "https://example.com:22/",
            "http://example.com:65535/",
            "http://no-such-host.invalid:25/",
        ] {
            let error = within(f.fetch_text(url)).await.unwrap_err();
            assert_eq!(error.code(), ERR_OUTBOUND_INVALID_PORT, "{url}");
            let error = within(f.fetch_text_for("caller", url)).await.unwrap_err();
            assert_eq!(error.code(), ERR_OUTBOUND_INVALID_PORT, "{url}");
        }
        assert_eq!(f.snapshot().denied, 8);
        for url in [
            "http://example.com/",
            "https://example.com/",
            "http://example.com:80/",
            "https://example.com:443/",
        ] {
            f.check_port(&parse_url(url).unwrap()).unwrap();
        }
        let custom = Fetcher::new(FetchConfig {
            allowed_ports: vec![8080],
            ..Default::default()
        })
        .unwrap();
        custom
            .check_port(&parse_url("http://example.com:8080/").unwrap())
            .unwrap();
        assert!(custom
            .check_port(&parse_url("http://example.com/").unwrap())
            .is_err());

        // An exception names its own port, so an odd port needs no list entry.
        let (origin, server) =
            fixture(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello")
                .await;
        let f = Fetcher::new(FetchConfig {
            internal_origins: vec![origin.clone()],
            ..Default::default()
        })
        .unwrap();
        assert_eq!(within(f.fetch_text(&origin)).await.unwrap(), "hello");
        within(server).await.unwrap();
        // Only that exact origin is exempt.
        assert_eq!(
            within(f.fetch_text("http://127.0.0.1:1/"))
                .await
                .unwrap_err()
                .code(),
            ERR_OUTBOUND_INVALID_PORT
        );
    }

    #[test]
    fn port_read_timeout_and_caller_settings_are_validated() {
        let bad_ports: [Vec<u16>; 3] = [vec![], vec![80, 0], (1..=17).collect()];
        for allowed_ports in bad_ports {
            assert!(FetchConfig {
                allowed_ports,
                ..Default::default()
            }
            .validate()
            .is_err());
        }
        assert!(FetchConfig {
            allowed_ports: (1..=16).collect(),
            ..Default::default()
        }
        .validate()
        .is_ok());
        for config in [
            FetchConfig {
                read_timeout_secs: 0,
                ..Default::default()
            },
            FetchConfig {
                read_timeout_secs: 301,
                ..Default::default()
            },
            FetchConfig {
                max_concurrent_per_caller: 0,
                ..Default::default()
            },
        ] {
            assert!(config.validate().is_err());
        }
        // The defaults are capped to a smaller timeout or concurrency, not rejected.
        assert!(FetchConfig {
            timeout_secs: 1,
            max_concurrent: 1,
            ..Default::default()
        }
        .validate()
        .is_ok());
    }

    #[tokio::test]
    async fn numeric_and_odd_host_forms_are_address_denied() {
        let f = Fetcher::new(FetchConfig::default()).unwrap();
        for url in [
            "http://2130706433/",
            "http://0x7f.1/",
            "http://127.1/",
            "http://[::1]/",
            "http://[::ffff:7f00:1]/",
            "http://[64:ff9b::7f00:1]/",
            "http://0.0.0.0/",
            "http://255.255.255.255/",
        ] {
            let error = within(f.fetch_text(url)).await.unwrap_err();
            assert_eq!(error.code(), ERR_OUTBOUND_ADDRESS_DENIED, "{url}");
        }
    }

    #[tokio::test]
    async fn stalled_body_is_cut_by_the_read_timeout_and_releases_the_permit() {
        let (origin, server) = stalling_origin(
            b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nab",
        )
        .await;
        let f = Fetcher::new(FetchConfig {
            internal_origins: vec![origin.clone()],
            max_concurrent: 1,
            timeout_secs: 30,
            read_timeout_secs: 1,
            ..Default::default()
        })
        .unwrap();
        let started = std::time::Instant::now();
        let error = within(f.fetch_text(&origin)).await.unwrap_err();
        assert_eq!(error.code(), ERR_OUTBOUND_TIMEOUT);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert_eq!(f.admission.available_permits(), 1);
        assert_eq!(f.snapshot().timed_out, 1);
        server.abort();
    }

    #[tokio::test]
    async fn per_caller_cap_isolates_callers_and_releases_slots() {
        let (stalled, held) = stalling_origin(b"").await;
        let (other_origin, other_server) =
            fixture(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello")
                .await;
        let (again_origin, again_server) =
            fixture(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello")
                .await;
        let f = Fetcher::new(FetchConfig {
            internal_origins: vec![stalled.clone(), other_origin.clone(), again_origin.clone()],
            max_concurrent: 4,
            max_concurrent_per_caller: 1,
            timeout_secs: 20,
            ..Default::default()
        })
        .unwrap();
        let worker = {
            let f = f.clone();
            let stalled = stalled.clone();
            tokio::spawn(async move { f.fetch_text_for("a", &stalled).await })
        };
        // The request reaches the stalled origin only once the slot is held.
        let mut waited = 0;
        while f.callers_len() == 0 {
            tokio::time::sleep(Duration::from_millis(10)).await;
            waited += 1;
            assert!(waited < 1000, "the first fetch never started");
        }
        let busy = within(f.fetch_text_for("a", &stalled)).await.unwrap_err();
        assert_eq!(busy.code(), ERR_OUTBOUND_BUSY);
        assert_eq!(f.callers_len(), 1);
        // Another caller, and the anonymous path, are not affected.
        assert_eq!(
            within(f.fetch_text_for("b", &other_origin)).await.unwrap(),
            "hello"
        );
        within(other_server).await.unwrap();
        assert_eq!(f.callers_len(), 1);
        // Cancelling the first fetch frees the slot and empties the table.
        worker.abort();
        let _ = worker.await;
        assert_eq!(f.callers_len(), 0);
        assert_eq!(f.admission.available_permits(), 4);
        assert_eq!(
            within(f.fetch_text_for("a", &again_origin)).await.unwrap(),
            "hello"
        );
        within(again_server).await.unwrap();
        assert_eq!(f.callers_len(), 0);
        assert_eq!(f.snapshot().busy, 1);
        held.abort();
    }

    #[test]
    fn link_local_and_unspecified_exceptions_need_an_explicit_flag() {
        for origin in [
            "http://169.254.169.254",
            "http://169.254.169.254:8080",
            "http://[fe80::1]",
            "http://0.0.0.0:8080",
            "http://[::]:8080",
            "http://[::ffff:169.254.169.254]",
        ] {
            let refused = FetchConfig {
                internal_origins: vec![origin.into()],
                ..Default::default()
            };
            assert!(refused.validate().is_err(), "{origin}");
            assert!(Fetcher::new(refused).is_err(), "{origin}");
            let accepted = FetchConfig {
                internal_origins: vec![origin.into()],
                allow_link_local_exceptions: true,
                ..Default::default()
            };
            assert!(accepted.validate().is_ok(), "{origin}");
            assert!(Fetcher::new(accepted).is_ok(), "{origin}");
        }
        for origin in [
            "http://127.0.0.1:8088",
            "http://10.0.0.5:8080",
            "http://[::1]:9000",
            "http://localhost:8123",
        ] {
            assert!(
                FetchConfig {
                    internal_origins: vec![origin.into()],
                    ..Default::default()
                }
                .validate()
                .is_ok(),
                "{origin}"
            );
        }
    }
}
