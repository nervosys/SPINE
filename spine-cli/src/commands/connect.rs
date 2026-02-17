//! `spine connect` — Interactive session with a SPINE server

use anyhow::Result;
use colored::Colorize;
use spine_agent::AgentClient;
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

pub async fn run(
    addr: String,
    ws: bool,
    tls: bool,
    ca: Option<PathBuf>,
    domain: Option<String>,
) -> Result<()> {
    eprintln!("{} Connecting to {}...", "▸".green().bold(), addr.cyan());

    if ws {
        let url = if addr.starts_with("ws://") || addr.starts_with("wss://") {
            addr.clone()
        } else {
            format!("ws://{}", addr)
        };
        let mut client = AgentClient::connect_ws(&url).await?;
        eprintln!(
            "{} Connected via WebSocket to {}",
            "✓".green().bold(),
            url.cyan()
        );
        interactive_session_ws(&mut client).await
    } else if tls {
        let domain_str = domain.as_deref().unwrap_or("localhost");
        let ca_path = ca.as_deref();
        let mut client = AgentClient::connect_tls(&addr, domain_str, ca_path, None).await?;
        let latency = client.ping().await?;
        eprintln!(
            "{} Connected via TLS ({}ms latency)",
            "✓".green().bold(),
            latency
        );
        interactive_session_tls(&mut client).await
    } else {
        let mut client = AgentClient::<TcpStream>::connect(&addr).await?;
        let latency = client.ping().await?;
        eprintln!("{} Connected ({}ms latency)", "✓".green().bold(), latency);
        interactive_session(&mut client).await
    }
}

async fn interactive_session(client: &mut AgentClient<TcpStream>) -> Result<()> {
    print_help();
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        print!("{} ", "spine>".cyan().bold());
        stdout.flush()?;

        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        match handle_command(line) {
            Cmd::Navigate(url) => match client.navigate(&url).await {
                Ok(()) => eprintln!("  {} Navigated to {}", "✓".green(), url),
                Err(e) => eprintln!("  {} {}", "✗".red(), e),
            },
            Cmd::Ur => match client.get_ur().await {
                Ok(ur) => {
                    println!("{}", serde_json::to_string_pretty(&ur)?);
                }
                Err(e) => eprintln!("  {} {}", "✗".red(), e),
            },
            Cmd::Html => match client.get_raw_html().await {
                Ok(html) => println!("{}", html),
                Err(e) => eprintln!("  {} {}", "✗".red(), e),
            },
            Cmd::Search(q) => match client.search(&q).await {
                Ok(val) => println!("{}", serde_json::to_string_pretty(&val)?),
                Err(e) => eprintln!("  {} {}", "✗".red(), e),
            },
            Cmd::Ping => match client.ping().await {
                Ok(ms) => eprintln!("  pong: {}ms", ms),
                Err(e) => eprintln!("  {} {}", "✗".red(), e),
            },
            Cmd::Exec(script) => match client.execute_hls(&script).await {
                Ok(result) => println!("{:?}", result),
                Err(e) => eprintln!("  {} {}", "✗".red(), e),
            },
            Cmd::Stats => {
                let s = client.get_speculation_stats();
                println!("Speculation Stats:");
                println!(
                    "  Output: {} predictions, {} hits ({:.1}%)",
                    s.output_predictions,
                    s.output_hits,
                    s.output_accuracy() * 100.0
                );
                println!(
                    "  Input:  {} predictions, {} hits ({:.1}%)",
                    s.input_predictions,
                    s.input_hits,
                    s.input_accuracy() * 100.0
                );
                println!("  Bytes saved: {}", s.bytes_saved);
            }
            Cmd::Morph => match client.morph().await {
                Ok(()) => eprintln!("  {} Protocol morphed", "✓".green()),
                Err(e) => eprintln!("  {} {}", "✗".red(), e),
            },
            Cmd::Capabilities => match client.get_capabilities().await {
                Ok(caps) => {
                    println!("Capabilities:");
                    for c in &caps {
                        println!("  • {}", c);
                    }
                }
                Err(e) => eprintln!("  {} {}", "✗".red(), e),
            },
            Cmd::Help => print_help(),
            Cmd::Quit => break,
            Cmd::Unknown(s) => eprintln!("  Unknown command: {}. Type 'help' for usage.", s),
        }
    }

    eprintln!("{} Disconnected", "▸".yellow());
    Ok(())
}

// TLS and WS sessions work the same way but with different client types.
// We use a macro to avoid duplicating the interactive loop for each transport.
async fn interactive_session_tls(client: &mut AgentClient<TlsStream<TcpStream>>) -> Result<()> {
    print_help();
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        print!("{} ", "spine(tls)>".cyan().bold());
        stdout.flush()?;

        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        match handle_command(line) {
            Cmd::Navigate(url) => match client.navigate(&url).await {
                Ok(()) => eprintln!("  {} Navigated to {}", "✓".green(), url),
                Err(e) => eprintln!("  {} {}", "✗".red(), e),
            },
            Cmd::Ur => match client.get_ur().await {
                Ok(ur) => println!("{}", serde_json::to_string_pretty(&ur)?),
                Err(e) => eprintln!("  {} {}", "✗".red(), e),
            },
            Cmd::Ping => match client.ping().await {
                Ok(ms) => eprintln!("  pong: {}ms", ms),
                Err(e) => eprintln!("  {} {}", "✗".red(), e),
            },
            Cmd::Help => print_help(),
            Cmd::Quit => break,
            _ => eprintln!("  Command not yet supported in TLS mode"),
        }
    }

    eprintln!("{} Disconnected", "▸".yellow());
    Ok(())
}

async fn interactive_session_ws(
    client: &mut AgentClient<spine_agent::WebSocketClientStream>,
) -> Result<()> {
    print_help();
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        print!("{} ", "spine(ws)>".cyan().bold());
        stdout.flush()?;

        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        match handle_command(line) {
            Cmd::Navigate(url) => match client.navigate(&url).await {
                Ok(()) => eprintln!("  {} Navigated to {}", "✓".green(), url),
                Err(e) => eprintln!("  {} {}", "✗".red(), e),
            },
            Cmd::Ur => match client.get_ur().await {
                Ok(ur) => println!("{}", serde_json::to_string_pretty(&ur)?),
                Err(e) => eprintln!("  {} {}", "✗".red(), e),
            },
            Cmd::Ping => match client.ping().await {
                Ok(ms) => eprintln!("  pong: {}ms", ms),
                Err(e) => eprintln!("  {} {}", "✗".red(), e),
            },
            Cmd::Help => print_help(),
            Cmd::Quit => break,
            _ => eprintln!("  Command not yet supported in WebSocket mode"),
        }
    }

    eprintln!("{} Disconnected", "▸".yellow());
    Ok(())
}

enum Cmd {
    Navigate(String),
    Ur,
    Html,
    Search(String),
    Ping,
    Exec(String),
    Stats,
    Morph,
    Capabilities,
    Help,
    Quit,
    Unknown(String),
}

fn handle_command(line: &str) -> Cmd {
    let parts: Vec<&str> = line.splitn(2, ' ').collect();
    match parts[0].to_lowercase().as_str() {
        "nav" | "navigate" | "go" => {
            if parts.len() > 1 {
                Cmd::Navigate(parts[1].to_string())
            } else {
                Cmd::Unknown("navigate requires a URL".into())
            }
        }
        "ur" | "get" => Cmd::Ur,
        "html" | "raw" => Cmd::Html,
        "search" | "find" => {
            if parts.len() > 1 {
                Cmd::Search(parts[1].to_string())
            } else {
                Cmd::Unknown("search requires a query".into())
            }
        }
        "ping" => Cmd::Ping,
        "exec" | "run" => {
            if parts.len() > 1 {
                Cmd::Exec(parts[1].to_string())
            } else {
                Cmd::Unknown("exec requires a script".into())
            }
        }
        "stats" => Cmd::Stats,
        "morph" => Cmd::Morph,
        "caps" | "capabilities" => Cmd::Capabilities,
        "help" | "?" | "h" => Cmd::Help,
        "quit" | "exit" | "q" => Cmd::Quit,
        other => Cmd::Unknown(other.into()),
    }
}

fn print_help() {
    eprintln!();
    eprintln!("{}", "Commands:".bold());
    eprintln!("  {}           Navigate to a URL", "nav <url>".cyan());
    eprintln!(
        "  {}                   Get Unified Representation",
        "ur".cyan()
    );
    eprintln!("  {}                 Get raw HTML", "html".cyan());
    eprintln!("  {}       Search the semantic web", "search <q>".cyan());
    eprintln!("  {}                 Ping the server", "ping".cyan());
    eprintln!("  {}         Execute HLS script", "exec <code>".cyan());
    eprintln!("  {}                Speculation stats", "stats".cyan());
    eprintln!("  {}                Morph the protocol", "morph".cyan());
    eprintln!("  {}                 Server capabilities", "caps".cyan());
    eprintln!("  {}                 Show this help", "help".cyan());
    eprintln!("  {}                 Disconnect", "quit".cyan());
    eprintln!();
}
