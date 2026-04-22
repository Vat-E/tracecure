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
    Serve {
        #[arg(long, default_value = "./data/sift.ndjson")]
        data: PathBuf,
        #[arg(long, default_value = "127.0.0.1:7789")]
        bind: SocketAddr,
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
        Cmd::Serve { data, bind, ui_dist } => {
            let state = sift_server::AppState::open(&data)
                .await
                .with_context(|| format!("opening data file at {}", data.display()))?;
            let app = sift_server::router_with_ui(state, ui_dist.clone());

            tracing::info!(%bind, data = %data.display(), ui = ?ui_dist, "starting sift-server");
            let listener = tokio::net::TcpListener::bind(bind).await?;
            axum::serve(listener, app).await?;
        }
    }
    Ok(())
}
