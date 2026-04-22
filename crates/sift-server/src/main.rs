use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "sift-server", version, about = "Sift log platform")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Run the HTTP server.
    Serve {
        /// Path to the NDJSON data file. Created if missing.
        #[arg(long, default_value = "./data/sift.ndjson")]
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
                .unwrap_or_else(|_| EnvFilter::new("info,sift_server=debug")),
        )
        .init();

    let cli = Cli::parse();

    match cli.cmd {
        Cmd::Serve {
            data,
            bind,
            ui_dist: _,
        } => {
            let state = sift_server::AppState::open(&data)
                .await
                .with_context(|| format!("opening data file at {}", data.display()))?;
            let app = sift_server::router(state);

            tracing::info!(%bind, data = %data.display(), "starting sift-server");
            let listener = tokio::net::TcpListener::bind(bind).await?;
            axum::serve(listener, app).await?;
        }
    }

    Ok(())
}
