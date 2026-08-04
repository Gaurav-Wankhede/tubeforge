//! clap CLI definition (LLD §4.1).
//!
//! Global flags: `--json`, `--verbose`, `--db-path`, `--config <env file>`.
//! Subcommands: `init`, `ingest channels/links`, `refresh`, `score`, `ideas`,
//! `keywords add|check|report`, `scorecard`, `health`, `alerts`, `reindex`,
//! `backup`, `quota`, `mcp`.

use std::path::PathBuf;

use clap::{ArgAction, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "tubeforge",
    version,
    about = "Local-first YouTube SEO/GEO growth engine"
)]
pub struct Cli {
    /// Machine-readable JSON envelope on stdout (LLD §4.2).
    #[arg(long, global = true)]
    pub json: bool,

    /// Verbose tracing output (sets LOG_LEVEL=debug).
    #[arg(long, global = true)]
    pub verbose: bool,

    /// Override database path (overrides TUBEFORGE_DB_PATH).
    #[arg(long, global = true, value_name = "PATH")]
    pub db_path: Option<PathBuf>,

    /// Load environment from this file instead of `.env`.
    #[arg(long, global = true, value_name = "FILE")]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create data root, .env scaffold, DB + migrations, test open.
    Init,
    /// Ingest channels (RSS + optional API) or video links (oEmbed/API).
    Ingest {
        #[command(subcommand)]
        kind: IngestKind,
    },
    /// Re-fetch known channels (ETag-aware; 304 = skipped).
    Refresh {
        /// Restrict to these channel ids (default: all known channels).
        #[arg(long, value_name = "ID")]
        channel: Vec<String>,
        /// Skip the automatic backup before the write batch.
        #[arg(long, action = ArgAction::SetTrue)]
        no_backup: bool,
    },
    /// Score a draft or stored video: full SEO+GEO weighted engine (LLD §7).
    Score {
        /// Stored video id to score.
        #[arg(long, value_name = "ID")]
        video_id: Option<String>,
        /// Draft title.
        #[arg(long, value_name = "TITLE")]
        draft_title: Option<String>,
        /// Draft description.
        #[arg(long, value_name = "DESC")]
        draft_desc: Option<String>,
        /// Draft tags (space or comma separated).
        #[arg(long, value_name = "TAGS")]
        draft_tags: Option<String>,
        /// Target keywords (repeatable; default: tracked keywords or the title).
        #[arg(long, value_name = "KW")]
        keywords: Vec<String>,
    },
    /// Generate + rank Next Ideas (LLD §8.2); --status marks the pool.
    Ideas {
        /// Max ideas returned (default 10).
        #[arg(long, value_name = "N", default_value_t = 10)]
        limit: usize,
        /// User niche terms for idea-fit scoring.
        #[arg(long, value_name = "NICHE")]
        niche: Option<String>,
        /// Mark the generated pool and filter: draft|saved|discarded.
        #[arg(long, value_name = "STATUS")]
        status: Option<String>,
    },
    /// Track keyword positions in the corpus (LLD §8.3).
    Keywords {
        #[command(subcommand)]
        kind: KeywordsKind,
    },
    /// Competitor comparison vs the median of the set (LLD §8.4).
    Scorecard {
        /// Restrict to these channel ids (default: competitors).
        #[arg(long, value_name = "ID")]
        channel: Vec<String>,
    },
    /// Data completeness, quota, integrity, freshness (LLD §8.4).
    Health,
    /// Brand/coverage/quota/integrity alerts (LLD §8.4).
    Alerts {
        #[command(subcommand)]
        action: Option<AlertsAction>,
        /// Mark all alerts as read after evaluating/listing.
        #[arg(long, action = ArgAction::SetTrue)]
        mark_read: bool,
    },
    /// Rebuild the tantivy index from the videos table (idempotent).
    Reindex,
    /// VACUUM INTO snapshot + integrity_check + retention prune.
    Backup {
        /// Snapshot directory (overrides TUBEFORGE_BACKUP_DIR).
        #[arg(long, value_name = "DIR")]
        to: Option<PathBuf>,
    },
    /// Show YouTube API usage from the meta ledger.
    Quota,
    /// Print the MCP server config snippet (tursodb <db> --mcp).
    Mcp,
}

#[derive(Debug, Subcommand)]
pub enum IngestKind {
    /// Resolve + fetch + upsert channels (RSS, optional API).
    Channels {
        /// Channel references: UC... id, URL, or @handle.
        #[arg(value_name = "REF", required = true)]
        refs: Vec<String>,
        /// Enrich with the YouTube Data API (requires YOUTUBE_API_KEY).
        #[arg(long, action = ArgAction::SetTrue)]
        api: bool,
        /// Skip the automatic backup before the write batch.
        #[arg(long, action = ArgAction::SetTrue)]
        no_backup: bool,
    },
    /// Read multi-line video URLs from stdin (`--file -`) or a file.
    Links {
        /// Input file; `-` or absent = stdin.
        #[arg(long, value_name = "FILE")]
        file: Option<String>,
        /// Enrich with the YouTube Data API (requires YOUTUBE_API_KEY).
        #[arg(long, action = ArgAction::SetTrue)]
        api: bool,
        /// Skip the automatic backup before the write batch.
        #[arg(long, action = ArgAction::SetTrue)]
        no_backup: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum KeywordsKind {
    /// Track keywords for ranking and scoring.
    Add {
        #[arg(value_name = "KW", required = true)]
        keywords: Vec<String>,
    },
    /// Snapshot corpus ranks for every tracked keyword.
    Check,
    /// Trend report across snapshots (deltas computed in Rust).
    Report,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum AlertsAction {
    /// List alerts without re-evaluating the rules.
    List,
    /// Delete all alerts.
    Clear,
}

impl Cli {
    pub fn command_name(&self) -> &'static str {
        match &self.command {
            Command::Init => "init",
            Command::Ingest { kind } => match kind {
                IngestKind::Channels { .. } => "ingest channels",
                IngestKind::Links { .. } => "ingest links",
            },
            Command::Refresh { .. } => "refresh",
            Command::Score { .. } => "score",
            Command::Ideas { .. } => "ideas",
            Command::Keywords { kind } => match kind {
                KeywordsKind::Add { .. } => "keywords add",
                KeywordsKind::Check => "keywords check",
                KeywordsKind::Report => "keywords report",
            },
            Command::Scorecard { .. } => "scorecard",
            Command::Health => "health",
            Command::Alerts { action, .. } => match action {
                Some(AlertsAction::List) => "alerts list",
                Some(AlertsAction::Clear) => "alerts clear",
                None => "alerts",
            },
            Command::Reindex => "reindex",
            Command::Backup { .. } => "backup",
            Command::Quota => "quota",
            Command::Mcp => "mcp",
        }
    }
}
