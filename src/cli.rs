//! clap CLI definition (LLD §4.1).
//!
//! Global flags: `--json`, `--verbose`, `--db-path`, `--config <env file>`.
//! Subcommands: `init`, `ingest channels/links`, `refresh`, `score`, `ideas`,
//! `keywords add|check|report`, `scorecard`, `health`, `alerts`, `reindex`,
//! `backup`, `quota`, `mcp`, `thumbnail render|list-templates`,
//! `check availability`, `export`, `filmot get`.

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
    /// Generate thumbnails from HTML+Tailwind templates (Phase 3, PRD §5.7).
    Thumbnail {
        #[command(subcommand)]
        kind: ThumbnailKind,
    },
    /// Detect tracked videos that went private/deleted; snapshot
    /// privacyStatus (Phase 3 workstream B).
    Check {
        #[command(subcommand)]
        kind: CheckKind,
    },
    /// Export the local dataset (CSVs + JSON arrays) as a zip or directory.
    Export {
        /// Output directory (zip archive lands here in --format zip).
        #[arg(long, value_name = "DIR", required = true)]
        out: PathBuf,
        /// zip (default) or plain dir.
        #[arg(long, value_name = "FORMAT", default_value = "zip")]
        format: ExportFormat,
    },
    /// Opt-in lookups against third-party indexes (Phase 3 workstream B).
    Filmot {
        #[command(subcommand)]
        kind: FilmotKind,
    },
    /// Serve the local HTMX dashboard (PRD §5.4 deferred item).
    ///
    /// Long-running server: binds loopback only, never emits the JSON
    /// envelope (stdout stays empty; the listening line goes to stderr).
    Serve {
        /// Listen port (default 8080; TUBEFORGE_SERVE_PORT overrides).
        #[arg(long, value_name = "PORT")]
        port: Option<u16>,
        /// Bind host — loopback only (127.0.0.1, localhost or ::1).
        #[arg(long, value_name = "HOST", default_value = "127.0.0.1")]
        host: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum CheckKind {
    /// Check stored videos for privacy/deletion via videos.list (needs
    /// YOUTUBE_API_KEY); missing videos raise `video_unavailable` alerts.
    Availability {
        /// Restrict the check to these video ids (default: all stored).
        #[arg(long, value_name = "ID")]
        video_id: Vec<String>,
    },
}

/// `--format` for `export` (parsed case-insensitively by clap).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ExportFormat {
    Zip,
    Dir,
}

#[derive(Debug, Subcommand)]
pub enum FilmotKind {
    /// Archived metadata for one video from the Filmot index (own API key).
    Get {
        /// The 11-char YouTube video id.
        #[arg(long, value_name = "ID", required = true)]
        video_id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ThumbnailKind {
    /// Render a 1280x720 thumbnail PNG via headless Chromium.
    Render {
        /// Stored video id to render (title, channel, duration, category).
        #[arg(long, value_name = "ID")]
        video_id: Option<String>,
        /// Draft title to render (no stored video needed).
        #[arg(long, value_name = "TITLE")]
        draft_title: Option<String>,
        /// Template name (default: "default").
        #[arg(long, value_name = "NAME", default_value = "default")]
        template: String,
        /// Output PNG path (default: <cwd>/<video_id>.png).
        #[arg(long, value_name = "PATH")]
        out: Option<PathBuf>,
        /// Keep the temporary assets dir (debug only; PRD §5.7 cleanup).
        #[arg(long, action = ArgAction::SetTrue)]
        keep_assets: bool,
    },
    /// List the available template names.
    ListTemplates,
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
            Command::Thumbnail { kind } => match kind {
                ThumbnailKind::Render { .. } => "thumbnail render",
                ThumbnailKind::ListTemplates => "thumbnail list-templates",
            },
            Command::Check { kind } => match kind {
                CheckKind::Availability { .. } => "check availability",
            },
            Command::Export { .. } => "export",
            Command::Filmot { kind } => match kind {
                FilmotKind::Get { .. } => "filmot get",
            },
            Command::Serve { .. } => "serve",
        }
    }
}
