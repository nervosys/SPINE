use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use tokio_rustls::rustls::{Certificate, PrivateKey, ServerConfig};
use tokio_rustls::TlsAcceptor;

pub fn load_certs(path: &Path) -> anyhow::Result<Vec<Certificate>> {
    let certfile = File::open(path)?;
    let mut reader = BufReader::new(certfile);
    let certs = rustls_pemfile::certs(&mut reader)?
        .into_iter()
        .map(Certificate)
        .collect();
    Ok(certs)
}

pub fn load_private_key(path: &Path) -> anyhow::Result<PrivateKey> {
    let keyfile = File::open(path)?;
    let mut reader = BufReader::new(keyfile);
    let keys = rustls_pemfile::pkcs8_private_keys(&mut reader)?;
    if keys.is_empty() {
        return Err(anyhow::anyhow!("No private key found"));
    }
    Ok(PrivateKey(keys[0].clone()))
}

pub fn create_tls_acceptor(cert_path: &Path, key_path: &Path, ca_path: Option<&Path>) -> anyhow::Result<TlsAcceptor> {
    let certs = load_certs(cert_path)?;
    let key = load_private_key(key_path)?;

    let builder = ServerConfig::builder()
        .with_safe_defaults();

    let config = if let Some(ca_path) = ca_path {
        let ca_certs = load_certs(ca_path)?;
        let mut roots = tokio_rustls::rustls::RootCertStore::empty();
        for cert in ca_certs {
            roots.add(&cert)?;
        }
        let client_auth = tokio_rustls::rustls::server::AllowAnyAuthenticatedClient::new(roots);
        builder.with_client_cert_verifier(Arc::new(client_auth))
            .with_single_cert(certs, key)?
    } else {
        builder.with_no_client_auth()
            .with_single_cert(certs, key)?
    };

    Ok(TlsAcceptor::from(Arc::new(config)))
}
