use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "tracecure-server", version, about = "Tracecure log platform")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Run the HTTP server.
    Serve {
        /// Path to the data directory. WAL + segment files live here.
        #[arg(long, default_value = "./data")]
        data: PathBuf,

        /// Bind address.
        #[arg(long, default_value = "127.0.0.1:7789")]
        bind: SocketAddr,

        /// Path to a pre-built UI dist directory (from `trunk build`). If
        /// absent, UI serving is disabled and `/` returns 404.
        #[arg(long)]
        ui_dist: Option<PathBuf>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,tracecure_server=debug")),
        )
        .init();

    let cli = Cli::parse();

    match cli.cmd {
        Cmd::Serve {
            data,
            bind,
            ui_dist,
        } => {
            let state = tracecure_server::AppState::open(&data)
                .await
                .with_context(|| format!("opening data dir at {}", data.display()))?;
            let app = tracecure_server::router_with_ui(state, ui_dist.clone());

            tracing::info!(%bind, data = %data.display(), ui = ?ui_dist, "starting tracecure-server");
            let listener = tokio::net::TcpListener::bind(bind).await?;
            axum::serve(listener, app).await?;
        }
    }
    Ok(())
}
