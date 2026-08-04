//! TubeForge binary entry point: tokio runtime, config load, clap dispatch,
//! error -> exit code mapping, JSON envelope vs human output (LLD §4).

use std::time::Instant;

use clap::Parser;
use tubeforge::cli::{Cli, Command, IngestKind, KeywordsKind};
use tubeforge::commands;
use tubeforge::config;
use tubeforge::error::TubeforgeError;
use tubeforge::fetch::quota::{self, DAILY_LIMIT};
use tubeforge::output::{self, Envelope, QuotaInfo};
use tubeforge::storage::Db;

#[tokio::main]
async fn main() {
    // clap usage errors exit(2) themselves (LLD §4.3).
    let cli = Cli::parse();
    let start = Instant::now();

    init_tracing(&cli);

    let exit_code = run(cli, start).await;
    std::process::exit(exit_code);
}

fn init_tracing(cli: &Cli) {
    use tracing_subscriber::EnvFilter;
    let level = if cli.verbose {
        "debug".to_string()
    } else {
        std::env::var("LOG_LEVEL").unwrap_or_else(|_| "info".into())
    };
    let filter = EnvFilter::try_new(&level).unwrap_or_else(|_| EnvFilter::new("info"));
    // Logs go to stderr so stdout stays a clean data channel (LLD §4.2: the
    // JSON envelope is the ONLY thing on stdout in --json mode).
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .try_init();
}

/// Full command pipeline. Returns the process exit code.
async fn run(cli: Cli, start: Instant) -> i32 {
    let json = cli.json;
    let result = dispatch(&cli).await;
    let duration_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok((data, quota_opt)) => {
            let envelope = Envelope::ok(data, Some(output::meta(duration_ms, quota_opt)));
            output::render(&envelope, json);
            0
        }
        Err(err) => {
            // LLD §4.4: errors always render in the JSON envelope; human mode
            // prints `code: message` on stderr.
            if json {
                let envelope = Envelope::error(&err);
                output::render(&envelope, true);
            } else {
                eprintln!("{}: {}", err.code(), err);
            }
            i32::from(&err)
        }
    }
}

async fn dispatch(cli: &Cli) -> Result<(serde_json::Value, Option<QuotaInfo>), TubeforgeError> {
    let cfg = config::load(cli.config.as_deref(), cli.db_path.as_deref())?;
    tracing::debug!(db_path = %cfg.db_path.display(), "config loaded");

    let data = match &cli.command {
        Command::Init => commands::init::run(&cfg).await?,
        Command::Ingest { kind } => match kind {
            IngestKind::Channels { refs, api, no_backup } => {
                commands::ingest::run_channels(&cfg, refs, *api, *no_backup).await?
            }
            IngestKind::Links { file, api, no_backup } => {
                commands::ingest::run_links(&cfg, file.clone(), *api, *no_backup).await?
            }
        },
        Command::Refresh { channel, no_backup } => {
            commands::refresh::run(&cfg, channel, *no_backup).await?
        }
        Command::Score {
            video_id,
            draft_title,
            draft_desc,
            draft_tags,
            keywords,
        } => {
            commands::score::run_with_keywords(
                &cfg,
                &commands::score::ScoreInput {
                    video_id: video_id.clone(),
                    draft_title: draft_title.clone(),
                    draft_desc: draft_desc.clone(),
                    draft_tags: draft_tags.clone(),
                },
                keywords,
            )
            .await?
        }
        Command::Ideas {
            limit,
            niche,
            status,
        } => commands::ideas::run(&cfg, *limit, niche.as_deref(), status.as_deref()).await?,
        Command::Keywords { kind } => {
            let db = Db::open(&cfg.db_path).await?;
            match kind {
                KeywordsKind::Add { keywords } => commands::keywords::run_add(&db, keywords).await?,
                KeywordsKind::Check => commands::keywords::run_check(&cfg).await?,
                KeywordsKind::Report => commands::keywords::run_report(&db).await?,
            }
        }
        Command::Scorecard { channel } => commands::scorecard::run(&cfg, channel).await?,
        Command::Health => commands::health::run(&cfg).await?,
        Command::Alerts { action, mark_read } => {
            commands::alerts::run(&cfg, *action, *mark_read).await?
        }
        Command::Reindex => commands::reindex::run(&cfg).await?,
        Command::Backup { to } => commands::backup::run(&cfg, to.clone()).await?,
        Command::Quota => commands::quota::run(&cfg).await?,
        Command::Mcp => commands::mcp::run(&cfg).await?,
    };

    // Attach the quota ledger to `meta.quota` for commands that touch the
    // YouTube API (LLD §4.2 envelope contract).
    let quota_opt = match &cli.command {
        Command::Ingest { .. } | Command::Refresh { .. } | Command::Quota => {
            quota_meta(&cfg).await
        }
        _ => None,
    };

    Ok((data, quota_opt))
}

/// Read the current videos.list ledger for the envelope `meta.quota`.
async fn quota_meta(cfg: &config::Config) -> Option<QuotaInfo> {
    match Db::open(&cfg.db_path).await {
        Ok(db) => match quota::used(&db).await {
            Ok((used, _)) => Some(QuotaInfo {
                videos_list_used: used,
                daily_limit: DAILY_LIMIT,
            }),
            Err(_) => None,
        },
        Err(_) => None,
    }
}
