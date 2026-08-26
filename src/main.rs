//! TubeForge binary entry point: tokio runtime, config load, clap dispatch,
//! error -> exit code mapping, JSON envelope vs human output (LLD §4).

use std::time::Instant;

use clap::Parser;
use tubeforge::cli::{
    CheckKind, Cli, Command, CommentsKind, ExportFormat as CliExportFormat, FilmotKind, GreedyKind,
    IngestKind, KanbanKind, KeywordsKind, SeedsAction, TagsKind, ThumbnailKind, TranscriptKind,
};
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
    // `serve` and `rpc` are long-running processes: they never emit the JSON
    // envelope (LLD §4.2 stdout purity — `serve` keeps stdout empty, `rpc`
    // reserves stdout for JSON-RPC responses), so they are dispatched outside
    // the envelope pipeline entirely.
    match &cli.command {
        Command::Serve { .. } => return run_serve(&cli).await,
        Command::Rpc => return run_rpc(&cli).await,
        _ => {}
    }
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

/// `serve` bootstrap: config load + command run; stdout untouched, exit 0
/// on clean shutdown (Ctrl-C), error line on stderr otherwise.
async fn run_serve(cli: &Cli) -> i32 {
    let cfg = match config::load(cli.config.as_deref(), cli.db_path.as_deref()) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("{}: {}", err.code(), err);
            return i32::from(&err);
        }
    };
    let (host, port) = match &cli.command {
        Command::Serve { host, port } => (host.clone(), *port),
        _ => unreachable!("run_serve is only called for Command::Serve"),
    };
    match commands::serve::run(&cfg, &host, port).await {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("{}: {}", err.code(), err);
            i32::from(&err)
        }
    }
}

/// `rpc` bootstrap: config load + stdio JSON-RPC bridge; stdout is reserved
/// for RPC responses, all diagnostics go to stderr, exit 0 on stdin EOF.
async fn run_rpc(cli: &Cli) -> i32 {
    let cfg = match config::load(cli.config.as_deref(), cli.db_path.as_deref()) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("{}: {}", err.code(), err);
            return i32::from(&err);
        }
    };
    match commands::rpc::run(&cfg).await {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("{}: {}", err.code(), err);
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
            IngestKind::Channels {
                refs,
                api,
                no_backup,
            } => commands::ingest::run_channels(&cfg, refs, *api, *no_backup).await?,
            IngestKind::Links {
                file,
                api,
                no_backup,
            } => commands::ingest::run_links(&cfg, file.clone(), *api, *no_backup).await?,
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
                KeywordsKind::Add { keywords } => {
                    commands::keywords::run_add(&db, keywords).await?
                }
                KeywordsKind::Check => commands::keywords::run_check(&cfg).await?,
                KeywordsKind::Report => commands::keywords::run_report(&db).await?,
                KeywordsKind::Inspect { keyword, serp } => {
                    commands::keywords::run_inspect(&cfg, keyword, *serp).await?
                }
                KeywordsKind::Research {
                    topics,
                    serp,
                    dedupe,
                } => commands::keywords::run_research(&cfg, topics, *serp, *dedupe).await?,
                KeywordsKind::Discover {
                    topic,
                    serp,
                    enrich,
                    transcripts,
                } => {
                    commands::keywords::run_discover(&cfg, topic, *serp, *enrich, *transcripts)
                        .await?
                }
            }
        }
        Command::Scorecard { channel } => commands::scorecard::run(&cfg, channel).await?,
        Command::Tags { kind } => match kind {
            TagsKind::Backfill => commands::tags::run_backfill(&cfg).await?,
            TagsKind::Analyze => commands::tags::run_analyze(&cfg).await?,
        },
        Command::Transcript { kind } => match kind {
            TranscriptKind::Get { video_id, lang } => {
                commands::transcript::run_get(&cfg, video_id, lang).await?
            }
            TranscriptKind::List => commands::transcript::run_list(&cfg).await?,
            TranscriptKind::Clear => commands::transcript::run_clear(&cfg).await?,
        },
        Command::Metadata { video_id } => commands::metadata::run(&cfg, video_id).await?,
        Command::Comments { kind } => match kind {
            CommentsKind::Get { video_id, max, api } => {
                commands::comments::run_get(&cfg, video_id, *max, *api).await?
            }
            CommentsKind::List { video_id } => commands::comments::run_list(&cfg, video_id).await?,
            CommentsKind::Clear => commands::comments::run_clear(&cfg).await?,
        },
        Command::Gaps { channel, markdown } => {
            commands::gaps::run_gaps(&cfg, channel, *markdown).await?
        }
        Command::Outliers { channel } => commands::gaps::run_outliers(&cfg, channel).await?,
        Command::Health => commands::health::run(&cfg).await?,
        Command::Analyze { topic, serp } => commands::analyze::run(&cfg, topic, *serp).await?,
        Command::Forecast {
            keyword,
            horizon,
            channels,
        } => {
            commands::forecast::run_forecast(&cfg, keyword.as_deref(), *horizon, *channels).await?
        }
        Command::Suggest { topic, horizon } => {
            commands::forecast::run_suggest(&cfg, topic, *horizon).await?
        }
        Command::Alerts { action, mark_read } => {
            commands::alerts::run(&cfg, *action, *mark_read).await?
        }
        Command::Reindex => commands::reindex::run(&cfg).await?,
        Command::Backup { to } => commands::backup::run(&cfg, to.clone()).await?,
        Command::Quota => commands::quota::run(&cfg).await?,
        Command::Thumbnail { kind } => match kind {
            ThumbnailKind::Render {
                video_id,
                draft_title,
                template,
                out,
                keep_assets,
            } => {
                commands::thumbnail::run_render(
                    &cfg,
                    &commands::thumbnail::RenderInput {
                        video_id: video_id.clone(),
                        draft_title: draft_title.clone(),
                        template: template.clone(),
                        out: out.clone(),
                        keep_assets: *keep_assets,
                    },
                )
                .await?
            }
            ThumbnailKind::ListTemplates => commands::thumbnail::run_list_templates().await?,
        },
        Command::Check { kind } => match kind {
            CheckKind::Availability { video_id } => {
                commands::availability::run(&cfg, video_id).await?
            }
        },
        Command::VideosDedupe => commands::videos::run_dedupe(&cfg).await?,
        Command::Export { out, format } => {
            let fmt = match format {
                CliExportFormat::Zip => commands::export::ExportFormat::Zip,
                CliExportFormat::Dir => commands::export::ExportFormat::Dir,
            };
            commands::export::run(&cfg, out, fmt).await?
        }
        Command::Filmot { kind } => match kind {
            FilmotKind::Get { video_id } => commands::filmot::run_get(video_id).await?,
        },
        Command::Prompt {
            video_id,
            multi,
            comments,
            out,
            json,
        } => {
            commands::prompt::run(
                &cfg,
                video_id.as_deref(),
                multi,
                *comments,
                out.as_ref(),
                *json,
            )
            .await?
        }
        Command::Greedy { kind } => match kind {
            GreedyKind::Run { max } => commands::greedy::run_research(&cfg, *max).await?,
            GreedyKind::Status => commands::greedy::run_status(&cfg).await?,
            GreedyKind::Seeds { action } => {
                let db = Db::open(&cfg.db_path).await?;
                match action {
                    SeedsAction::Add { seeds } => {
                        commands::greedy::run_seeds_add(&db, seeds).await?
                    }
                    SeedsAction::List => commands::greedy::run_seeds_list(&db).await?,
                    SeedsAction::Deactivate { seed_id } => {
                        commands::greedy::run_seeds_deactivate(&db, *seed_id).await?
                    }
                    SeedsAction::Init => commands::greedy::run_seeds_init(&db).await?,
                }
            }
            GreedyKind::Daemon { interval, max } => {
                commands::greedy::run_daemon(&cfg, *interval, *max).await?
            }
            GreedyKind::Stop => commands::greedy::run_stop(&cfg).await?,
        },
        Command::Kanban { kind } => match kind {
            KanbanKind::Create {
                title,
                channel,
                status,
                topic,
                framework,
                duration,
                keyword,
                youtube_url,
                notes,
            } => {
                commands::kanban::run_create(
                    &cfg,
                    &commands::kanban::CreateTicketInput {
                        title: title.clone(),
                        channel: channel.clone(),
                        status: status.clone(),
                        topic: topic.clone(),
                        framework: framework.clone(),
                        optimal_duration_sec: *duration,
                        target_keyword: keyword.clone(),
                        youtube_url: youtube_url.clone(),
                        notes: notes.clone(),
                    },
                )
                .await?
            }
            KanbanKind::FromResearch {
                topic,
                channel,
                title,
                framework,
                duration,
            } => {
                commands::kanban::run_from_research(
                    &cfg,
                    topic,
                    channel,
                    title.as_deref(),
                    framework.as_deref(),
                    *duration,
                )
                .await?
            }
            KanbanKind::List { status, channel } => {
                commands::kanban::run_list(&cfg, status.as_deref(), channel.as_deref()).await?
            }
            KanbanKind::Move {
                ticket_id,
                status,
                youtube_url,
                video_id,
            } => {
                commands::kanban::run_move(
                    &cfg,
                    ticket_id,
                    status,
                    youtube_url.as_deref(),
                    video_id.as_deref(),
                )
                .await?
            }
            KanbanKind::Update {
                ticket_id,
                title,
                status,
                topic,
                framework,
                duration,
                keyword,
                notes,
            } => {
                commands::kanban::run_update(
                    &cfg,
                    ticket_id,
                    title.as_deref(),
                    status.as_deref(),
                    topic.as_deref(),
                    framework.as_deref(),
                    *duration,
                    keyword.as_deref(),
                    notes.as_deref(),
                )
                .await?
            }
            KanbanKind::Show { ticket_id } => commands::kanban::run_show(&cfg, ticket_id).await?,
            KanbanKind::Delete { ticket_id } => {
                commands::kanban::run_delete(&cfg, ticket_id).await?
            }
            KanbanKind::Prompt { ticket_id } => {
                commands::kanban::run_prompt(&cfg, ticket_id).await?
            }
        },
        // Serve never reaches the envelope pipeline (special-cased in run()).
        Command::Serve { .. } => unreachable!("serve is handled before dispatch"),
        // Rpc is long-running stdio; also special-cased in run().
        Command::Rpc => unreachable!("rpc is handled before dispatch"),
    };

    // Attach the quota ledger to `meta.quota` for commands that touch the
    // YouTube API (LLD §4.2 envelope contract).
    let quota_opt = match &cli.command {
        Command::Ingest { .. }
        | Command::Refresh { .. }
        | Command::Quota
        | Command::Check { .. }
        | Command::Comments {
            kind: CommentsKind::Get { api: true, .. },
        } => quota_meta(&cfg).await,
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
