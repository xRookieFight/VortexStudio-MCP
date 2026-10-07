use rmcp::ServiceExt;
use rmcp::transport::stdio;
use vortexstudio_mcp::server::VortexStudio;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("--version" | "-V") => {
            println!("vortexstudio-mcp {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("--help" | "-h") => {
            println!(
                "vortexstudio-mcp {}\nMCP server for Vortex Studio. It talks MCP over stdin and stdout,\n\
                 so add it to your MCP client instead of running it by hand.",
                env!("CARGO_PKG_VERSION")
            );
            return Ok(());
        }
        _ => {}
    }
    // stdout belongs to the protocol, anything human goes to stderr
    let service = VortexStudio::new().serve(stdio()).await.inspect_err(|e| {
        eprintln!("failed to start: {e}");
    })?;
    service.waiting().await?;
    Ok(())
}
