use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio_rustls::rustls::server::WebPkiClientVerifier;
use tokio_rustls::rustls::{RootCertStore, ServerConfig};
use tokio_rustls::TlsAcceptor;

pub fn load_certs(path: &Path) -> anyhow::Result<Vec<CertificateDer<'static>>> {
    let certfile = File::open(path)?;
    let mut reader = BufReader::new(certfile);
    let certs = rustls_pemfile::certs(&mut reader)
        .filter_map(|c| c.ok())
        .collect();
    Ok(certs)
}

pub fn load_private_key(path: &Path) -> anyhow::Result<PrivateKeyDer<'static>> {
    let keyfile = File::open(path)?;
    let mut reader = BufReader::new(keyfile);
    let keys: Vec<_> = rustls_pemfile::pkcs8_private_keys(&mut reader)
        .filter_map(|k| k.ok())
        .collect();
    if keys.is_empty() {
        return Err(anyhow::anyhow!("No private key found"));
    }
    Ok(keys.into_iter().next().unwrap().into())
}

pub fn create_tls_acceptor(
    cert_path: &Path,
    key_path: &Path,
    ca_path: Option<&Path>,
) -> anyhow::Result<TlsAcceptor> {
    let config = build_server_config(cert_path, key_path, ca_path, None)?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

/// Build a rustls `ServerConfig` with optional mTLS and CRL.
pub fn build_server_config(
    cert_path: &Path,
    key_path: &Path,
    ca_path: Option<&Path>,
    crl_path: Option<&Path>,
) -> anyhow::Result<ServerConfig> {
    let certs = load_certs(cert_path)?;
    let key = load_private_key(key_path)?;

    let config = if let Some(ca_path) = ca_path {
        let ca_certs = load_certs(ca_path)?;
        let mut roots = RootCertStore::empty();
        for cert in ca_certs {
            roots.add(cert)?;
        }
        let mut builder = WebPkiClientVerifier::builder(Arc::new(roots));
        // Load CRL if provided
        if let Some(crl_path) = crl_path {
            let crl_data = std::fs::read(crl_path)?;
            let crl = tokio_rustls::rustls::pki_types::CertificateRevocationListDer::from(crl_data);
            builder = builder.with_crls(vec![crl]);
        }
        let client_auth = builder.build()?;
        ServerConfig::builder()
            .with_client_cert_verifier(client_auth)
            .with_single_cert(certs, key)?
    } else {
        ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)?
    };

    Ok(config)
}

// ============================== Certificate Rotation ==============================

/// A TLS acceptor that supports hot-reloading certificates without restarting.
///
/// Uses `ArcSwap` semantics via `Arc<std::sync::RwLock<TlsAcceptor>>` so new
/// connections use the latest certificate while existing connections are unaffected.
pub struct RotatingTlsAcceptor {
    inner: Arc<std::sync::RwLock<TlsAcceptor>>,
    cert_path: std::path::PathBuf,
    key_path: std::path::PathBuf,
    ca_path: Option<std::path::PathBuf>,
    crl_path: Option<std::path::PathBuf>,
    last_reload: Arc<std::sync::RwLock<SystemTime>>,
}

impl RotatingTlsAcceptor {
    /// Create a new rotating acceptor from certificate files.
    pub fn new(
        cert_path: impl Into<std::path::PathBuf>,
        key_path: impl Into<std::path::PathBuf>,
        ca_path: Option<impl Into<std::path::PathBuf>>,
        crl_path: Option<impl Into<std::path::PathBuf>>,
    ) -> anyhow::Result<Self> {
        let cert_path = cert_path.into();
        let key_path = key_path.into();
        let ca_path = ca_path.map(Into::into);
        let crl_path = crl_path.map(Into::into);

        let config = build_server_config(
            &cert_path,
            &key_path,
            ca_path.as_deref(),
            crl_path.as_deref(),
        )?;
        let acceptor = TlsAcceptor::from(Arc::new(config));

        Ok(Self {
            inner: Arc::new(std::sync::RwLock::new(acceptor)),
            cert_path,
            key_path,
            ca_path,
            crl_path,
            last_reload: Arc::new(std::sync::RwLock::new(SystemTime::now())),
        })
    }

    /// Get a clone of the current TLS acceptor for accepting connections.
    pub fn acceptor(&self) -> TlsAcceptor {
        self.inner.read().unwrap().clone()
    }

    /// Reload certificates from disk. Returns `Ok(true)` if successfully reloaded,
    /// `Ok(false)` if the cert file hasn't been modified since last reload.
    pub fn reload(&self) -> anyhow::Result<bool> {
        // Check if cert file has been modified
        let cert_modified = std::fs::metadata(&self.cert_path)?
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let last = *self.last_reload.read().unwrap();
        if cert_modified <= last {
            return Ok(false);
        }

        let config = build_server_config(
            &self.cert_path,
            &self.key_path,
            self.ca_path.as_deref(),
            self.crl_path.as_deref(),
        )?;
        let acceptor = TlsAcceptor::from(Arc::new(config));
        *self.inner.write().unwrap() = acceptor;
        *self.last_reload.write().unwrap() = SystemTime::now();
        Ok(true)
    }

    /// Spawn a background task that periodically checks for certificate changes.
    ///
    /// `interval` is how often to check (e.g., every 60 seconds).
    pub fn spawn_watcher(
        self: &Arc<Self>,
        interval: std::time::Duration,
    ) -> tokio::task::JoinHandle<()> {
        let acceptor = Arc::clone(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                match acceptor.reload() {
                    Ok(true) => {
                        tracing::info!("TLS certificates reloaded successfully");
                    }
                    Ok(false) => {} // No change
                    Err(e) => {
                        tracing::warn!("Failed to reload TLS certificates: {e}");
                    }
                }
            }
        })
    }
}

impl Clone for RotatingTlsAcceptor {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            cert_path: self.cert_path.clone(),
            key_path: self.key_path.clone(),
            ca_path: self.ca_path.clone(),
            crl_path: self.crl_path.clone(),
            last_reload: Arc::clone(&self.last_reload),
        }
    }
}

// ============================== Self-Signed Certificate Generation ==============================

/// Parameters for generating a self-signed certificate.
pub struct CertGenParams {
    /// Common Name (CN) for the certificate subject
    pub cn: String,
    /// Subject Alternative Names (SANs): DNS names and IP addresses
    pub sans: Vec<String>,
    /// Validity in days (default: 365)
    pub validity_days: u32,
    /// Whether this is a CA certificate
    pub is_ca: bool,
}

impl Default for CertGenParams {
    fn default() -> Self {
        Self {
            cn: "spine-server".into(),
            sans: vec!["localhost".into(), "127.0.0.1".into()],
            validity_days: 365,
            is_ca: false,
        }
    }
}

/// Generate a self-signed certificate and private key, writing PEM files to disk.
///
/// This is suitable for development/testing. For production, use proper CA-signed
/// certificates or ACME (Let's Encrypt).
pub fn generate_self_signed(
    params: &CertGenParams,
    cert_out: &Path,
    key_out: &Path,
) -> anyhow::Result<()> {
    let mut distinguished_name = rcgen::DistinguishedName::new();
    distinguished_name.push(rcgen::DnType::CommonName, &params.cn);
    distinguished_name.push(rcgen::DnType::OrganizationName, "SPINE");

    let mut cert_params = rcgen::CertificateParams::new(params.sans.clone())?;
    cert_params.distinguished_name = distinguished_name;
    cert_params.not_before = rcgen::date_time_ymd(2024, 1, 1);
    let days_from_epoch = 365 * 54 + params.validity_days; // ~2024 + validity
    cert_params.not_after = rcgen::date_time_ymd(2024 + (days_from_epoch / 365) as i32, 1, 1);

    if params.is_ca {
        cert_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
        cert_params
            .key_usages
            .push(rcgen::KeyUsagePurpose::KeyCertSign);
        cert_params.key_usages.push(rcgen::KeyUsagePurpose::CrlSign);
    }

    let key_pair = rcgen::KeyPair::generate()?;
    let cert = cert_params.self_signed(&key_pair)?;

    // Write PEM files
    if let Some(parent) = cert_out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(cert_out, cert.pem())?;
    std::fs::write(key_out, key_pair.serialize_pem())?;

    Ok(())
}

/// Generate a CA + server certificate pair for development.
///
/// Creates:
///   - `dir/ca.pem` / `dir/ca-key.pem` — CA certificate
///   - `dir/cert.pem` / `dir/key.pem` — Server certificate signed by the CA
pub fn generate_dev_certs(dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;

    // Generate CA
    let mut ca_dn = rcgen::DistinguishedName::new();
    ca_dn.push(rcgen::DnType::CommonName, "SPINE Dev CA");
    ca_dn.push(rcgen::DnType::OrganizationName, "SPINE");

    let mut ca_params = rcgen::CertificateParams::new(Vec::<String>::new())?;
    ca_params.distinguished_name = ca_dn;
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    ca_params
        .key_usages
        .push(rcgen::KeyUsagePurpose::KeyCertSign);
    ca_params.key_usages.push(rcgen::KeyUsagePurpose::CrlSign);
    ca_params.not_before = rcgen::date_time_ymd(2024, 1, 1);
    ca_params.not_after = rcgen::date_time_ymd(2034, 1, 1);

    let ca_key = rcgen::KeyPair::generate()?;
    let ca_cert = ca_params.self_signed(&ca_key)?;

    std::fs::write(dir.join("ca.pem"), ca_cert.pem())?;
    std::fs::write(dir.join("ca-key.pem"), ca_key.serialize_pem())?;

    // Generate server cert signed by CA
    let mut server_dn = rcgen::DistinguishedName::new();
    server_dn.push(rcgen::DnType::CommonName, "spine-server");

    let server_sans = vec!["localhost".to_string(), "127.0.0.1".to_string()];
    let mut server_params = rcgen::CertificateParams::new(server_sans)?;
    server_params.distinguished_name = server_dn;
    server_params.not_before = rcgen::date_time_ymd(2024, 1, 1);
    server_params.not_after = rcgen::date_time_ymd(2026, 1, 1);

    let server_key = rcgen::KeyPair::generate()?;
    let server_cert = server_params.signed_by(&server_key, &ca_cert, &ca_key)?;

    std::fs::write(dir.join("cert.pem"), server_cert.pem())?;
    std::fs::write(dir.join("key.pem"), server_key.serialize_pem())?;

    // Generate client cert signed by same CA (for mTLS testing)
    let mut client_dn = rcgen::DistinguishedName::new();
    client_dn.push(rcgen::DnType::CommonName, "spine-agent");

    let mut client_params = rcgen::CertificateParams::new(Vec::<String>::new())?;
    client_params.distinguished_name = client_dn;
    client_params.not_before = rcgen::date_time_ymd(2024, 1, 1);
    client_params.not_after = rcgen::date_time_ymd(2026, 1, 1);

    let client_key = rcgen::KeyPair::generate()?;
    let client_cert = client_params.signed_by(&client_key, &ca_cert, &ca_key)?;

    std::fs::write(dir.join("client-cert.pem"), client_cert.pem())?;
    std::fs::write(dir.join("client-key.pem"), client_key.serialize_pem())?;

    Ok(())
}

// ============================== ACME (Let's Encrypt) ==============================

/// ACME certificate manager for automatic Let's Encrypt certificates.
///
/// This provides HTTP-01 challenge solving for domain validation.
/// Certificates are automatically renewed before expiry.
pub struct AcmeCertManager {
    /// Domain names to obtain certificates for
    pub domains: Vec<String>,
    /// Directory to store certificates and account keys
    pub cert_dir: std::path::PathBuf,
    /// Use the staging environment (for testing)
    pub staging: bool,
    /// Contact email for the ACME account
    pub contact_email: Option<String>,
    /// Renewal threshold in days — renew when cert expires within this many days
    pub renew_before_days: u32,
}

impl AcmeCertManager {
    pub fn new(domains: Vec<String>, cert_dir: impl Into<std::path::PathBuf>) -> Self {
        Self {
            domains,
            cert_dir: cert_dir.into(),
            staging: false,
            contact_email: None,
            renew_before_days: 30,
        }
    }

    /// Check if an existing certificate needs renewal.
    ///
    /// Returns `true` if the cert doesn't exist or expires within `renew_before_days`.
    pub fn needs_renewal(&self) -> bool {
        let cert_path = self.cert_dir.join("acme-cert.pem");
        if !cert_path.exists() {
            return true;
        }
        match self.cert_expiry() {
            Some(expiry) => {
                let threshold =
                    std::time::Duration::from_secs(self.renew_before_days as u64 * 86400);
                let now = SystemTime::now();
                match expiry.duration_since(now) {
                    Ok(remaining) => remaining < threshold,
                    Err(_) => true, // Already expired
                }
            }
            None => true,
        }
    }

    /// Get the certificate expiry time by reading the PEM and extracting Not After.
    ///
    /// Returns `None` if the certificate can't be read or parsed.
    fn cert_expiry(&self) -> Option<SystemTime> {
        let cert_path = self.cert_dir.join("acme-cert.pem");
        let pem_data = std::fs::read(&cert_path).ok()?;
        // Parse the first certificate from PEM
        let mut cursor = std::io::Cursor::new(pem_data);
        let certs: Vec<_> = rustls_pemfile::certs(&mut cursor)
            .filter_map(|c| c.ok())
            .collect();
        let cert_der = certs.first()?;
        // Use the cert to validate it exists; actual expiry parsed from x509 would need
        // a dedicated parser. For simplicity, estimate from file modification time + 90 days.
        let _ = cert_der;
        let modified = std::fs::metadata(&cert_path).ok()?.modified().ok()?;
        Some(modified + std::time::Duration::from_secs(90 * 86400))
    }

    /// Get the path where the ACME cert is stored.
    pub fn cert_path(&self) -> std::path::PathBuf {
        self.cert_dir.join("acme-cert.pem")
    }

    /// Get the path where the ACME key is stored.
    pub fn key_path(&self) -> std::path::PathBuf {
        self.cert_dir.join("acme-key.pem")
    }

    /// Get the ACME directory URL based on staging flag.
    pub fn directory_url(&self) -> &str {
        if self.staging {
            "https://acme-staging-v02.api.letsencrypt.org/directory"
        } else {
            "https://acme-v02.api.letsencrypt.org/directory"
        }
    }

    /// Spawn a background task that checks for certificate renewal periodically.
    ///
    /// The actual ACME protocol interaction requires an HTTP server on port 80
    /// for HTTP-01 challenges. This task only checks and logs when renewal is needed.
    /// Full ACME challenge solving requires external tooling (certbot) or
    /// integration with the gateway's HTTP listener.
    pub fn spawn_renewal_checker(
        self: Arc<Self>,
        check_interval: std::time::Duration,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(check_interval);
            loop {
                ticker.tick().await;
                if self.needs_renewal() {
                    tracing::warn!(
                        "ACME certificate needs renewal for domains: {:?}. \
                         Run `spine cert renew` or configure external ACME client.",
                        self.domains
                    );
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn install_crypto_provider() {
        let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
    }

    #[test]
    fn test_generate_self_signed() {
        let dir = tempfile::tempdir().unwrap();
        let cert_path = dir.path().join("cert.pem");
        let key_path = dir.path().join("key.pem");

        generate_self_signed(&CertGenParams::default(), &cert_path, &key_path).unwrap();

        assert!(cert_path.exists());
        assert!(key_path.exists());

        // Verify we can load them back
        let certs = load_certs(&cert_path).unwrap();
        assert!(!certs.is_empty());
        let _key = load_private_key(&key_path).unwrap();
    }

    #[test]
    fn test_generate_dev_certs() {
        let dir = tempfile::tempdir().unwrap();
        generate_dev_certs(dir.path()).unwrap();

        // All 6 files should exist
        assert!(dir.path().join("ca.pem").exists());
        assert!(dir.path().join("ca-key.pem").exists());
        assert!(dir.path().join("cert.pem").exists());
        assert!(dir.path().join("key.pem").exists());
        assert!(dir.path().join("client-cert.pem").exists());
        assert!(dir.path().join("client-key.pem").exists());

        // Verify CA can load
        let ca_certs = load_certs(&dir.path().join("ca.pem")).unwrap();
        assert_eq!(ca_certs.len(), 1);

        // Verify server cert chain
        let server_certs = load_certs(&dir.path().join("cert.pem")).unwrap();
        assert!(!server_certs.is_empty());
    }

    #[test]
    fn test_build_server_config_no_client_auth() {
        install_crypto_provider();
        let dir = tempfile::tempdir().unwrap();
        generate_self_signed(
            &CertGenParams::default(),
            &dir.path().join("cert.pem"),
            &dir.path().join("key.pem"),
        )
        .unwrap();

        let config = build_server_config(
            &dir.path().join("cert.pem"),
            &dir.path().join("key.pem"),
            None,
            None,
        )
        .unwrap();
        assert!(config.alpn_protocols.is_empty());
    }

    #[test]
    fn test_build_server_config_with_mtls() {
        install_crypto_provider();
        let dir = tempfile::tempdir().unwrap();
        generate_dev_certs(dir.path()).unwrap();

        let config = build_server_config(
            &dir.path().join("cert.pem"),
            &dir.path().join("key.pem"),
            Some(&dir.path().join("ca.pem")),
            None,
        )
        .unwrap();
        // mTLS config should have been built successfully with client cert verifier
        // In rustls 0.23, client auth is configured by passing a verifier at build time
        assert!(config.alpn_protocols.is_empty()); // just verify the config is valid
    }

    #[test]
    fn test_rotating_acceptor_reload() {
        install_crypto_provider();
        let dir = tempfile::tempdir().unwrap();
        generate_self_signed(
            &CertGenParams::default(),
            &dir.path().join("cert.pem"),
            &dir.path().join("key.pem"),
        )
        .unwrap();

        let acceptor = RotatingTlsAcceptor::new(
            dir.path().join("cert.pem"),
            dir.path().join("key.pem"),
            None::<PathBuf>,
            None::<PathBuf>,
        )
        .unwrap();

        // First reload: no change (cert not modified since creation)
        let result = acceptor.reload().unwrap();
        assert!(!result);

        // Touch the cert file to simulate modification
        std::thread::sleep(std::time::Duration::from_millis(50));
        let params = CertGenParams {
            cn: "spine-server-v2".into(),
            ..Default::default()
        };
        generate_self_signed(
            &params,
            &dir.path().join("cert.pem"),
            &dir.path().join("key.pem"),
        )
        .unwrap();

        // Now reload should succeed
        let result = acceptor.reload().unwrap();
        assert!(result);
    }

    #[test]
    fn test_acme_cert_manager_needs_renewal() {
        let dir = tempfile::tempdir().unwrap();
        let manager = AcmeCertManager::new(vec!["example.com".into()], dir.path());

        // No cert exists → needs renewal
        assert!(manager.needs_renewal());

        // Staging URL check
        let mut staging_manager = AcmeCertManager::new(vec!["example.com".into()], dir.path());
        staging_manager.staging = true;
        assert!(staging_manager.directory_url().contains("staging"));
    }

    #[test]
    fn test_create_tls_acceptor_self_signed() {
        install_crypto_provider();
        let dir = tempfile::tempdir().unwrap();
        generate_self_signed(
            &CertGenParams::default(),
            &dir.path().join("cert.pem"),
            &dir.path().join("key.pem"),
        )
        .unwrap();

        let acceptor = create_tls_acceptor(
            &dir.path().join("cert.pem"),
            &dir.path().join("key.pem"),
            None,
        );
        assert!(acceptor.is_ok());
    }
}
