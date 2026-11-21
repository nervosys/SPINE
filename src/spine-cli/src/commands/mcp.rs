//! MCP stdio adapter to the native core's read-only tools.
use anyhow::Result;
use clap::Args;
use spine_agent::AgentClient;
use spine_protocol::{CapabilityAdvertisement, ToolCall, ToolOutcome, ToolResult};
use std::path::PathBuf;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

#[derive(Args)]
pub struct McpArgs {
    #[arg(long, default_value = "127.0.0.1:8080")]
    pub addr: String,
    #[arg(long)]
    pub tls: bool,
    #[arg(long, requires = "tls")]
    pub ca: Option<PathBuf>,
    #[arg(long, default_value = "localhost")]
    pub domain: String,
    #[arg(long, requires_all = ["tls", "client_key"])]
    pub client_cert: Option<PathBuf>,
    #[arg(long, requires_all = ["tls", "client_cert"])]
    pub client_key: Option<PathBuf>,
}

pub async fn run(args: McpArgs) -> Result<()> {
    if args.tls {
        let authentication = args.client_cert.as_deref().zip(args.client_key.as_deref());
        let client =
            AgentClient::connect_tls(&args.addr, &args.domain, args.ca.as_deref(), authentication)
                .await?;
        serve(client).await
    } else {
        serve(AgentClient::<TcpStream>::connect(&args.addr).await?).await
    }
}

// Stable aliases give MCP hosts ordinary tool identifiers. Native URI
// translation stays local; nothing else is delegated by this CLI.
fn alias(uri: &str) -> Option<&'static str> {
    match uri {
        "spine.content/hash" => Some("spine_content_hash"),
        "spine.html/parse" => Some("spine_html_parse"),
        _ => None,
    }
}

async fn serve<S>(mut client: AgentClient<S>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Send + Unpin + 'static,
{
    let mut advertisement: CapabilityAdvertisement = client.capabilities().await?;
    let mut names = std::collections::HashMap::new();
    advertisement.capabilities.retain_mut(|capability| {
        if let Some(name) = alias(&capability.uri) {
            names.insert(name.to_string(), capability.uri.clone());
            capability.uri = name.into();
            true
        } else {
            false
        }
    });
    let runtime = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let mut client = Some(client);
        let mut server =
            spine_protocol::mcp::McpServer::new(advertisement, move |call: ToolCall| {
                let failed = || ToolResult {
                    id: call.id.clone(),
                    trace: call.trace.clone(),
                    outcome: ToolOutcome::Err {
                        code: "native_connection_unavailable".into(),
                        message: "Native tool connection unavailable; restart the MCP process"
                            .into(),
                        details: None,
                    },
                };
                let Some(uri) = names.get(&call.name) else {
                    return failed();
                };
                let Some(connected) = client.as_mut() else {
                    return failed();
                };
                let result = runtime.block_on(async {
                    tokio::time::timeout(
                        std::time::Duration::from_secs(30),
                        connected.call_tool(uri, call.args.clone()),
                    )
                    .await
                });
                match result {
                    Ok(Ok(result)) => result,
                    _ => {
                        // A timeout may leave a response or partial frame outstanding.
                        // Never reuse that client or replay an uncertain request.
                        client = None;
                        failed()
                    }
                }
            });
        server.server_name = "spine-core-tools".into();
        spine_protocol::mcp::serve_stdio(
            &mut server,
            std::io::stdin().lock(),
            &mut std::io::stdout().lock(),
        )
    })
    .await??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mcp_aliases_expose_only_the_supported_read_only_tools() {
        assert_eq!(alias("spine.content/hash"), Some("spine_content_hash"));
        assert_eq!(alias("spine.html/parse"), Some("spine_html_parse"));
        assert!(alias("shell").is_none());
        assert!(alias("spine.resource/upload").is_none());
    }
}
