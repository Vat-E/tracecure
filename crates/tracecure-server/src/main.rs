use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand, ValueEnum};
use tracecure_server::auth::TokenStore;
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

        /// Whether `/v1/ingest` needs a token (see the `token` subcommand).
        /// Falls back to `TRACECURE_INGEST_AUTH`; default `off`.
        #[arg(long, value_enum, value_name = "MODE")]
        ingest_auth: Option<IngestAuth>,
    },

    /// Manage the tokens that let a client write to `/v1/ingest`.
    Token {
        #[command(subcommand)]
        cmd: TokenCmd,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum IngestAuth {
    /// Anyone who can reach the port can write. The default, and what every
    /// version before tokens did.
    Off,
    /// Every ingest request must carry `Authorization: Bearer <token>`.
    Required,
}

#[derive(Subcommand, Debug)]
enum TokenCmd {
    /// Create a token and print it. It is shown once and cannot be
    /// recovered; only its hash is stored.
    Create {
        /// What the token is for — a project or a host. Must be unique.
        #[arg(long)]
        name: String,
        /// The server's data directory.
        #[arg(long, default_value = "./data")]
        data: PathBuf,
    },
    /// List tokens: id, name and creation time. Never the secret.
    List {
        #[arg(long, default_value = "./data")]
        data: PathBuf,
    },
    /// Revoke a token by id. Takes effect on a running server at once.
    Revoke {
        id: String,
        #[arg(long, default_value = "./data")]
        data: PathBuf,
    },
}

/// The ingest-auth mode from the flag, else the environment, else off.
fn ingest_auth_from(
    flag: Option<IngestAuth>,
    env: impl Fn(&str) -> Option<String>,
) -> anyhow::Result<IngestAuth> {
    if let Some(mode) = flag {
        return Ok(mode);
    }
    match env("TRACECURE_INGEST_AUTH")
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        None => Ok(IngestAuth::Off),
        Some(value) => IngestAuth::from_str(value, true).map_err(|_| {
            anyhow::anyhow!("TRACECURE_INGEST_AUTH: \"{value}\" is not `off` or `required`")
        }),
    }
}

fn run_token_command(cmd: TokenCmd) -> anyhow::Result<()> {
    match cmd {
        TokenCmd::Create { name, data } => {
            let (record, token) = TokenStore::open(&data).create(&name)?;
            // The token alone on stdout, so it can be piped or captured;
            // everything a person needs to read goes to stderr.
            eprintln!(
                "Created ingest token \"{}\" (id {}). It is shown once — store it now.",
                record.name, record.id
            );
            println!("{token}");
        }
        TokenCmd::List { data } => {
            let tokens = TokenStore::open(&data).list()?;
            if tokens.is_empty() {
                eprintln!("No tokens.");
            }
            for t in tokens {
                println!(
                    "{}\t{}\t{}\t{}",
                    t.id,
                    t.scope,
                    t.created_at
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    t.name
                );
            }
        }
        TokenCmd::Revoke { id, data } => {
            if TokenStore::open(&data).revoke(&id)? {
                eprintln!("Revoked {id}.");
            } else {
                anyhow::bail!("no token with id \"{id}\"");
            }
        }
    }
    Ok(())
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
            ingest_auth,
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
            let mut state = tracecure_server::AppState::open_with(&data, config)
                .await
                .with_context(|| format!("opening data dir at {}", data.display()))?;

            match ingest_auth_from(ingest_auth, |k| std::env::var(k).ok())? {
                IngestAuth::Required => {
                    let tokens = TokenStore::open(&data);
                    // Unreadable at startup is a stop, not a warning: the
                    // server would otherwise come up rejecting every writer
                    // with nothing in the log to say why.
                    let count = tokens
                        .list()
                        .context("ingest auth is required but the token file cannot be read")?
                        .len();
                    if count == 0 {
                        tracing::warn!(
                            "ingest auth is required and there are no tokens: every ingest \
                             request will be rejected until one is created with \
                             `tracecure-server token create`"
                        );
                    } else {
                        tracing::info!(tokens = count, "ingest auth required");
                    }
                    state = state.require_ingest_token(tokens);
                }
                IngestAuth::Off => {
                    tracing::info!("ingest auth off: anyone who can reach the port can write");
                }
            }
            let app = tracecure_server::router_with_ui(state, ui_dist.clone());

            tracing::info!(%bind, data = %data.display(), ui = ?ui_dist, "starting tracecure-server");
            let listener = tokio::net::TcpListener::bind(bind).await?;
            axum::serve(listener, app).await?;
        }
        Cmd::Token { cmd } => run_token_command(cmd)?,
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
    fn ingest_auth_defaults_to_off() {
        assert_eq!(ingest_auth_from(None, no_env).unwrap(), IngestAuth::Off);
    }

    #[test]
    fn ingest_auth_reads_the_environment_and_the_flag_wins() {
        let env = |k: &str| (k == "TRACECURE_INGEST_AUTH").then(|| "Required".to_string());
        assert_eq!(ingest_auth_from(None, env).unwrap(), IngestAuth::Required);
        assert_eq!(
            ingest_auth_from(Some(IngestAuth::Off), env).unwrap(),
            IngestAuth::Off
        );
    }

    #[test]
    fn an_empty_ingest_auth_variable_means_off() {
        let env = |k: &str| (k == "TRACECURE_INGEST_AUTH").then(|| "  ".to_string());
        assert_eq!(ingest_auth_from(None, env).unwrap(), IngestAuth::Off);
    }

    /// A typo here must not silently leave ingest open.
    #[test]
    fn an_unknown_ingest_auth_value_stops_startup() {
        let env = |k: &str| (k == "TRACECURE_INGEST_AUTH").then(|| "requried".to_string());
        let e = ingest_auth_from(None, env).unwrap_err();
        assert!(e.to_string().contains("requried"), "{e}");
    }

    #[test]
    fn the_same_stream_twice_is_an_error_not_last_one_wins() {
        let e = retention_from(None, vec!["a=1d".into(), "a=2d".into()], no_env).unwrap_err();
        assert!(e.to_string().contains("more than once"), "{e}");
    }
}
