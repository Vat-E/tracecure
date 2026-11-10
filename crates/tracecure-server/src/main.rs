use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use tracecure_storage::retention::{parse_duration, parse_stream_rule};
use tracecure_storage::{RetentionConfig, StoreConfig};
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

        /// Delete records older than this: a whole number and a unit, e.g.
        /// 12h, 30d, 8w. Applies to every stream without a rule of its own.
        /// Unset = keep everything. Falls back to `TRACECURE_RETENTION`.
        #[arg(long, value_name = "DURATION")]
        retention: Option<String>,

        /// A different maximum age for one stream: `<stream_id>=<duration>`.
        /// Repeat for several. Overrides --retention for that stream, shorter
        /// or longer. Falls back to `TRACECURE_RETENTION_STREAMS`
        /// (comma-separated).
        #[arg(long = "retention-stream", value_name = "STREAM=DURATION")]
        retention_stream: Vec<String>,
    },
}

/// Build the retention rules from the flags, falling back to the environment
/// for whichever of the two was not given on the command line.
///
/// The environment is read here rather than through clap so that a
/// container can be configured without overriding its whole command line.
fn retention_from(
    default: Option<String>,
    streams: Vec<String>,
    env: impl Fn(&str) -> Option<String>,
) -> anyhow::Result<RetentionConfig> {
    let default = default
        .or_else(|| env("TRACECURE_RETENTION"))
        .filter(|s| !s.trim().is_empty());
    let streams = if streams.is_empty() {
        env("TRACECURE_RETENTION_STREAMS")
            .map(|s| {
                s.split(',')
                    .map(str::trim)
                    .filter(|r| !r.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    } else {
        streams
    };

    let mut cfg = RetentionConfig::default();
    if let Some(d) = default {
        cfg.default = Some(parse_duration(&d).map_err(|e| anyhow::anyhow!("--retention: {e}"))?);
    }
    for rule in streams {
        let (stream, age) =
            parse_stream_rule(&rule).map_err(|e| anyhow::anyhow!("--retention-stream: {e}"))?;
        if cfg.per_stream.insert(stream.clone(), age).is_some() {
            anyhow::bail!("--retention-stream: \"{stream}\" is given more than once");
        }
    }
    Ok(cfg)
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
            retention,
            retention_stream,
        } => {
            let retention = retention_from(retention, retention_stream, |k| std::env::var(k).ok())?;
            // Said out loud at startup: this is the setting that deletes
            // data, so what it is must never have to be inferred.
            if retention.is_enabled() {
                tracing::info!(
                    default = ?retention.default,
                    per_stream = ?retention.per_stream,
                    "retention enabled: records past these ages are deleted"
                );
            } else {
                tracing::info!("retention disabled: nothing is ever deleted");
            }

            let config = StoreConfig {
                retention,
                ..StoreConfig::default()
            };
            let state = tracecure_server::AppState::open_with(&data, config)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const DAY: Duration = Duration::from_secs(86_400);

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn no_flags_and_no_environment_means_no_retention() {
        let cfg = retention_from(None, vec![], no_env).unwrap();
        assert!(!cfg.is_enabled());
    }

    #[test]
    fn flags_set_the_default_and_the_stream_rules() {
        let cfg = retention_from(
            Some("30d".into()),
            vec!["noisy=1d".into(), "audit=52w".into()],
            no_env,
        )
        .unwrap();
        assert_eq!(cfg.default, Some(DAY * 30));
        assert_eq!(cfg.per_stream.get("noisy"), Some(&DAY));
        assert_eq!(cfg.per_stream.get("audit"), Some(&(DAY * 364)));
    }

    #[test]
    fn the_environment_fills_in_what_the_flags_left_out() {
        let env = |k: &str| match k {
            "TRACECURE_RETENTION" => Some("7d".to_string()),
            "TRACECURE_RETENTION_STREAMS" => Some("a=1d, b=2d ,".to_string()),
            _ => None,
        };
        let cfg = retention_from(None, vec![], env).unwrap();
        assert_eq!(cfg.default, Some(DAY * 7));
        assert_eq!(cfg.per_stream.len(), 2);
        assert_eq!(cfg.per_stream.get("b"), Some(&(DAY * 2)));
    }

    #[test]
    fn a_flag_wins_over_the_environment() {
        let env = |k: &str| match k {
            "TRACECURE_RETENTION" => Some("7d".to_string()),
            "TRACECURE_RETENTION_STREAMS" => Some("a=1d".to_string()),
            _ => None,
        };
        let cfg = retention_from(Some("30d".into()), vec!["z=3d".into()], env).unwrap();
        assert_eq!(cfg.default, Some(DAY * 30));
        assert_eq!(cfg.per_stream.keys().collect::<Vec<_>>(), vec!["z"]);
    }

    #[test]
    fn an_empty_environment_value_is_not_a_rule() {
        let env = |k: &str| (k == "TRACECURE_RETENTION").then(String::new);
        assert!(!retention_from(None, vec![], env).unwrap().is_enabled());
    }

    #[test]
    fn a_bad_value_stops_startup_and_names_the_flag() {
        let e = retention_from(Some("soon".into()), vec![], no_env).unwrap_err();
        assert!(e.to_string().starts_with("--retention:"), "{e}");
        let e = retention_from(None, vec!["nope".into()], no_env).unwrap_err();
        assert!(e.to_string().starts_with("--retention-stream:"), "{e}");
    }

    #[test]
    fn the_same_stream_twice_is_an_error_not_last_one_wins() {
        let e = retention_from(None, vec!["a=1d".into(), "a=2d".into()], no_env).unwrap_err();
        assert!(e.to_string().contains("more than once"), "{e}");
    }
}
