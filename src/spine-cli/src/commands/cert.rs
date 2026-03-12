//! `spine cert` — Certificate management commands

use anyhow::Result;
use colored::Colorize;
use std::path::PathBuf;

use crate::CertAction;

pub async fn run(action: CertAction) -> Result<()> {
    match action {
        CertAction::Generate { output } => generate(output).await,
        CertAction::Info { path } => info(path).await,
    }
}

async fn generate(output: PathBuf) -> Result<()> {
    eprintln!(
        "{} Generating development certificates in {}...",
        "▸".green().bold(),
        output.display()
    );

    std::fs::create_dir_all(&output)?;
    spine_core::tls::generate_dev_certs(&output)?;

    eprintln!("{} Generated certificates:", "✓".green().bold());
    eprintln!("  CA:     {}/ca_cert.pem", output.display());
    eprintln!("  Server: {}/server_cert.pem + server_key.pem", output.display());
    eprintln!("  Client: {}/client_cert.pem + client_key.pem", output.display());
    eprintln!();
    eprintln!("{}", "Usage:".bold());
    eprintln!(
        "  Server: spine deploy --config spine.toml  (set tls.cert_path/key_path/ca_path)"
    );
    eprintln!(
        "  Client: spine connect <addr> --tls --ca {}/ca_cert.pem --client-cert {}/client_cert.pem --client-key {}/client_key.pem",
        output.display(), output.display(), output.display()
    );

    Ok(())
}

async fn info(path: PathBuf) -> Result<()> {
    let pem_data = std::fs::read(&path)?;
    let certs: Vec<_> = rustls_pemfile::certs(&mut &pem_data[..]).collect();

    if certs.is_empty() {
        eprintln!("{} No certificates found in {}", "✗".red().bold(), path.display());
        return Ok(());
    }

    for (i, cert_result) in certs.iter().enumerate() {
        match cert_result {
            Ok(cert) => {
                eprintln!(
                    "{} Certificate {} ({} bytes DER)",
                    "▸".green().bold(),
                    i + 1,
                    cert.as_ref().len()
                );
            }
            Err(e) => {
                eprintln!(
                    "{} Certificate {} parse error: {}",
                    "✗".red().bold(),
                    i + 1,
                    e
                );
            }
        }
    }

    Ok(())
}
