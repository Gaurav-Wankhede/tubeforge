//! clap CLI definition (LLD §4.1).
//!
//! Global flags: `--json`, `--verbose`, `--db-path`, `--config <env file>`.
//! Subcommands: `init`, `ingest channels/links`, `refresh`, `score`, `ideas`,
//! `keywords add|check|report`, `scorecard`, `health`, `alerts`, `reindex`,
//! `backup`, `quota`, `thumbnail render|list-templates`,
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
    /// Tag analysis (LLD §8.3 extension): backfill normalized tag tables.
    Tags {
        #[command(subcommand)]
        kind: TagsKind,
    },
    /// Transcript extraction via yt-dlp (Phase 6.5 content layer).
    Transcript {
        #[command(subcommand)]
        kind: TranscriptKind,
    },
    /// yt-dlp metadata enrichment (Phase 6.6): heatmap (audience retention),
    /// live stats, channel followers — persisted for stored videos.
    Metadata {
        /// The 11-char YouTube video id.
        #[arg(long, value_name = "ID", required = true)]
        video_id: String,
    },
    /// Fetch top-level comments via commentThreads.list (quota-guarded).
    Comments {
        #[command(subcommand)]
        kind: CommentsKind,
    },
    /// Competitor gap mining (Phase 6.5): outliers + coverage map.
    Gaps {
        /// Restrict to these channel ids (default: all).
        #[arg(long, value_name = "ID")]
        channel: Vec<String>,
        /// Emit a markdown gap report instead of the JSON envelope.
        #[arg(long, action = ArgAction::SetTrue)]
        markdown: bool,
    },
    /// Outlier videos (≥3x channel mean views) — proven demand (Method A).
    Outliers {
        /// Restrict to these channel ids (default: all).
        #[arg(long, value_name = "ID")]
        channel: Vec<String>,
    },
    /// Competitor comparison vs the median of the set (LLD §8.4).
    Scorecard {
        /// Restrict to these channel ids (default: competitors).
        #[arg(long, value_name = "ID")]
        channel: Vec<String>,
    },
    /// Data completeness, quota, integrity, freshness (LLD §8.4).
    Health,
    /// Precise topic analysis for the own channel: realtime SERP scan +
    /// demand-supply gap + auto-drafted title/description/tags.
    Analyze {
        /// The topic to analyze.
        #[arg(value_name = "TOPIC", required = true)]
        topic: String,
        /// SERP size for the scan (default 6).
        #[arg(long, value_name = "N", default_value_t = 6)]
        serp: u64,
    },
    /// Future forecasting over stored keyword-research history (LLD §8 +
    /// forecast layer): time-series extrapolation of opportunity/competition/
    /// views → rising/flat/falling verdict + next-period estimate.
    Forecast {
        /// Optional keyword to forecast. Empty = forecast all researched keywords.
        #[arg(value_name = "KEYWORD")]
        keyword: Option<String>,
        /// Forecast horizon in days (default 7).
        #[arg(long, value_name = "DAYS", default_value_t = 7)]
        horizon: u64,
        /// Also forecast channel growth from channel_snapshots history.
        #[arg(long, action = ArgAction::SetTrue)]
        channels: bool,
    },
    /// Auto-draft Title / Description / Tags for a future video from TubeForge
    /// research + forecast data.
    Suggest {
        /// The topic to package (must be a researched keyword).
        #[arg(value_name = "TOPIC", required = true)]
        topic: String,
        /// Forecast horizon in days (default 7).
        #[arg(long, value_name = "DAYS", default_value_t = 7)]
        horizon: u64,
    },
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
    /// Collapse duplicate videos (same channel + title) into one record,
    /// repointing scores/tags/transcripts/comments to the winner.
    VideosDedupe,
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
    /// Build an AI gap-mining prompt bundle (Phase 6.5): transcript +
    /// metadata (+ comments) wrapped in the research templates. Output is a
    /// markdown file ready to paste into OpenCode / Claude Code / Codex.
    Prompt {
        /// Stored video id to mine.
        #[arg(long, value_name = "ID")]
        video_id: Option<String>,
        /// Multiple video ids → the Multi-Video Pattern template.
        #[arg(long, value_name = "ID", value_delimiter = ',')]
        multi: Vec<String>,
        /// Include stored comments (fetched via `comments get`).
        #[arg(long, action = ArgAction::SetTrue)]
        comments: bool,
        /// Output file (default: stdout).
        #[arg(long, value_name = "PATH")]
        out: Option<PathBuf>,
        /// Emit structured JSON instead of the markdown bundle.
        #[arg(long, action = ArgAction::SetTrue)]
        json: bool,
    },
    /// Automated greedy topic research on autopilot.
    Greedy {
        #[command(subcommand)]
        kind: GreedyKind,
    },
    /// Kanban ticket management for video production workflow and roadmap.
    Kanban {
        #[command(subcommand)]
        kind: KanbanKind,
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
    /// Serve JSON-RPC over stdio for agent harnesses (OpenCode, Claude Code,
    /// Codex, Hermes, Pi Agent, ...).
    ///
    /// Long-running bridge: reads one JSON-RPC request per stdin line, streams
    /// responses (progress/result/error) to stdout. stdout is reserved for
    /// responses — it never emits the JSON envelope.
    Rpc,
}

#[derive(Debug, Subcommand)]
pub enum KanbanKind {
    /// Create a new Kanban ticket manually.
    Create {
        /// Ticket title (e.g. "The 7 Levers of Influence Explained").
        #[arg(long, value_name = "TITLE")]
        title: String,
        /// Channel name (TECHVERSE, BOOKVERSE, etc.).
        #[arg(long, value_name = "CHANNEL")]
        channel: String,
        /// Initial status (todo, inprogress, done, published). Default: todo.
        #[arg(long, value_name = "STATUS")]
        status: Option<String>,
        /// Topic name (binds to keyword research).
        #[arg(long, value_name = "TOPIC")]
        topic: Option<String>,
        /// Core framework or mental model name.
        #[arg(long, value_name = "FRAMEWORK")]
        framework: Option<String>,
        /// Optimal target duration in seconds (e.g. 720 for 12m).
        #[arg(long, value_name = "SECS")]
        duration: Option<i64>,
        /// Target primary keyword.
        #[arg(long, value_name = "KW")]
        keyword: Option<String>,
        /// Published YouTube URL (if already uploaded).
        #[arg(long, value_name = "URL")]
        youtube_url: Option<String>,
        /// Execution or production notes.
        #[arg(long, value_name = "NOTES")]
        notes: Option<String>,
    },
    /// Create a ticket automatically mapped from existing keyword research.
    FromResearch {
        /// Research topic name (must exist in keyword_research or will be queried).
        #[arg(value_name = "TOPIC")]
        topic: String,
        /// Target channel (TECHVERSE or BOOKVERSE).
        #[arg(long, value_name = "CHANNEL")]
        channel: String,
        /// Custom title override.
        #[arg(long, value_name = "TITLE")]
        title: Option<String>,
        /// Framework / mental model name.
        #[arg(long, value_name = "FRAMEWORK")]
        framework: Option<String>,
        /// Target duration in seconds.
        #[arg(long, value_name = "SECS")]
        duration: Option<i64>,
    },
    /// List Kanban tickets.
    List {
        /// Filter by status (todo, inprogress, done, published).
        #[arg(long, value_name = "STATUS")]
        status: Option<String>,
        /// Filter by channel (TECHVERSE, BOOKVERSE).
        #[arg(long, value_name = "CHANNEL")]
        channel: Option<String>,
    },
    /// Move/transition a ticket's status.
    Move {
        /// Ticket ID (e.g. ticket-abc12345).
        #[arg(value_name = "TICKET_ID")]
        ticket_id: String,
        /// New status (todo, inprogress, done, published).
        #[arg(value_name = "STATUS")]
        status: String,
        /// Published YouTube URL (when marking published).
        #[arg(long, value_name = "URL")]
        youtube_url: Option<String>,
        /// Stored video ID in TubeForge corpus.
        #[arg(long, value_name = "ID")]
        video_id: Option<String>,
    },
    /// Update/edit a Kanban ticket's metadata (title, topic, framework, keyword, notes).
    Update {
        /// Ticket ID (e.g. ticket-abc12345).
        #[arg(value_name = "TICKET_ID")]
        ticket_id: String,
        /// New title.
        #[arg(long, value_name = "TITLE")]
        title: Option<String>,
        /// New status.
        #[arg(long, value_name = "STATUS")]
        status: Option<String>,
        /// New topic.
        #[arg(long, value_name = "TOPIC")]
        topic: Option<String>,
        /// New framework.
        #[arg(long, value_name = "FRAMEWORK")]
        framework: Option<String>,
        /// New duration in seconds.
        #[arg(long, value_name = "SECS")]
        duration: Option<i64>,
        /// New target keyword.
        #[arg(long, value_name = "KW")]
        keyword: Option<String>,
        /// New notes.
        #[arg(long, value_name = "NOTES")]
        notes: Option<String>,
    },
    /// Show full details of a ticket and its interconnected research.
    Show {
        /// Ticket ID.
        #[arg(value_name = "TICKET_ID")]
        ticket_id: String,
    },
    /// Delete a Kanban ticket.
    Delete {
        /// Ticket ID.
        #[arg(value_name = "TICKET_ID")]
        ticket_id: String,
    },
    /// Generate First-Screen contract production prompt blueprint for a ticket.
    Prompt {
        /// Ticket ID.
        #[arg(value_name = "TICKET_ID")]
        ticket_id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum CheckKind {
    /// Check stored videos for privacy/deletion via videos.list (needs
    /// YOUTUBE_API_KEY); missing videos raise `video_unavailable` alerts.
    Availability {
        /// Restrict the check to these video ids (default: all stored).
        video_id: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum GreedyKind {
    /// Generate candidates and research as many as possible.
    Run {
        /// Max topics to research this run (default 5).
        #[arg(long, value_name = "N", default_value_t = 5)]
        max: usize,
    },
    /// Stats on the research history.
    Status,
    /// Manage seed topics.
    Seeds {
        #[command(subcommand)]
        action: SeedsAction,
    },
    /// Run the greedy bot as a long-running daemon. Researches topics on a
    /// fixed interval, writes a PID file for clean shutdown.
    Daemon {
        /// Seconds between research runs (default 3600 = 1 hour).
        #[arg(long, value_name = "SECS", default_value_t = 3600)]
        interval: u64,
        /// Max topics per run (default 5).
        #[arg(long, value_name = "N", default_value_t = 5)]
        max: usize,
    },
    /// Stop a running greedy daemon (sends SIGTERM via PID file).
    Stop,
}

#[derive(Debug, Subcommand)]
pub enum SeedsAction {
    /// Add seed topics.
    Add {
        /// Seed keywords (repeatable).
        #[arg(value_name = "SEED")]
        seeds: Vec<String>,
    },
    /// List all seeds.
    List,
    /// Deactivate a seed by its numeric id.
    Deactivate {
        /// Seed id to deactivate.
        #[arg(value_name = "ID")]
        seed_id: i64,
    },
    /// Seed the table with default topics for the owner channel niche
    /// (Rust, WebAssembly, Linux, tooling). Skips seeds that already exist.
    Init,
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
    /// VidIQ-style research for one keyword: SERP demand proxy, competition,
    /// opportunity score, related keywords (keyless yt-dlp + autocomplete).
    Inspect {
        /// The keyword to research.
        #[arg(value_name = "KW", required = true)]
        keyword: String,
        /// SERP size for demand/competition analysis (default 10).
        #[arg(long, value_name = "N", default_value_t = 10)]
        serp: u64,
    },
    /// Batch research for many topics (loops `inspect`, persisting each).
    Research {
        /// Topics to research (repeatable).
        #[arg(value_name = "TOPIC", required = true)]
        topics: Vec<String>,
        /// SERP size per topic (default 6 — ~17s each).
        #[arg(long, value_name = "N", default_value_t = 6)]
        serp: u64,
        /// Dedupe videos after the batch (same channel + title → one record).
        #[arg(long, action = ArgAction::SetTrue)]
        dedupe: bool,
    },
    /// Dynamic search-driven discovery: top-ranking channels & videos for a
    /// searched topic, registered as competitors + enriched for trends.
    Discover {
        /// The topic to discover (the search text).
        #[arg(value_name = "TOPIC", required = true)]
        topic: String,
        /// SERP size (default 10 — top-ranking videos for the search).
        #[arg(long, value_name = "N", default_value_t = 10)]
        serp: u64,
        /// Fetch per-video retention heatmaps + live stats (slower, richer).
        #[arg(long, action = ArgAction::SetTrue)]
        enrich: bool,
        /// Also fetch per-video transcripts (feeds gap-mining prompts).
        #[arg(long, action = ArgAction::SetTrue)]
        transcripts: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum AlertsAction {
    /// List alerts without re-evaluating the rules.
    List,
    /// Delete all alerts.
    Clear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub enum TagsKind {
    /// Populate tags/video_tags/competitor_tags from stored video data.
    Backfill,
    /// Aggregate per-channel tag stats into competitor_tags (the gaps
    /// table). Run after backfill so the mapping tables are populated.
    Analyze,
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum TranscriptKind {
    /// Fetch + store one video's transcript (yt-dlp, public captions).
    Get {
        /// The 11-char YouTube video id.
        #[arg(long, value_name = "ID", required = true)]
        video_id: String,
        /// Caption language (default "en").
        #[arg(long, value_name = "LANG", default_value = "en")]
        lang: String,
    },
    /// List stored transcripts.
    List,
    /// Delete all stored transcripts.
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum CommentsKind {
    /// Fetch top-level comments (yt-dlp keyless default; `--api` uses the
    /// YouTube Data API instead — needs YOUTUBE_API_KEY).
    Get {
        /// The 11-char YouTube video id.
        #[arg(long, value_name = "ID", required = true)]
        video_id: String,
        /// Max comments (default 100 = one page).
        #[arg(long, value_name = "N", default_value_t = 100)]
        max: u64,
        /// Force the YouTube Data API path (needs YOUTUBE_API_KEY).
        #[arg(long, action = ArgAction::SetTrue)]
        api: bool,
    },
    /// List stored comments for one video.
    List {
        /// The 11-char YouTube video id.
        #[arg(long, value_name = "ID", required = true)]
        video_id: String,
    },
    /// Delete all stored comments.
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
                KeywordsKind::Inspect { .. } => "keywords inspect",
                KeywordsKind::Research { .. } => "keywords research",
                KeywordsKind::Discover { .. } => "keywords discover",
            },
            Command::Tags { kind } => match kind {
                TagsKind::Backfill => "tags backfill",
                TagsKind::Analyze => "tags analyze",
            },
            Command::Transcript { kind } => match kind {
                TranscriptKind::Get { .. } => "transcript get",
                TranscriptKind::List => "transcript list",
                TranscriptKind::Clear => "transcript clear",
            },
            Command::Metadata { .. } => "metadata",
            Command::Comments { kind } => match kind {
                CommentsKind::Get { .. } => "comments get",
                CommentsKind::List { .. } => "comments list",
                CommentsKind::Clear => "comments clear",
            },
            Command::Gaps { .. } => "gaps",
            Command::Outliers { .. } => "outliers",
            Command::Scorecard { .. } => "scorecard",
            Command::Health => "health",
            Command::Analyze { .. } => "analyze",
            Command::Forecast { .. } => "forecast",
            Command::Suggest { .. } => "suggest",
            Command::Alerts { action, .. } => match action {
                Some(AlertsAction::List) => "alerts list",
                Some(AlertsAction::Clear) => "alerts clear",
                None => "alerts",
            },
            Command::Reindex => "reindex",
            Command::Backup { .. } => "backup",
            Command::Quota => "quota",
            Command::Thumbnail { kind } => match kind {
                ThumbnailKind::Render { .. } => "thumbnail render",
                ThumbnailKind::ListTemplates => "thumbnail list-templates",
            },
            Command::Check { kind } => match kind {
                CheckKind::Availability { .. } => "check availability",
            },
            Command::VideosDedupe => "videos dedupe",
            Command::Export { .. } => "export",
            Command::Filmot { kind } => match kind {
                FilmotKind::Get { .. } => "filmot get",
            },
            Command::Prompt { .. } => "prompt",
            Command::Greedy { kind } => match kind {
                GreedyKind::Run { .. } => "greedy run",
                GreedyKind::Status => "greedy status",
                GreedyKind::Seeds { action } => match action {
                    SeedsAction::Add { .. } => "greedy seeds add",
                    SeedsAction::List => "greedy seeds list",
                    SeedsAction::Deactivate { .. } => "greedy seeds deactivate",
                    SeedsAction::Init => "greedy seeds init",
                },
                GreedyKind::Daemon { .. } => "greedy daemon",
                GreedyKind::Stop => "greedy stop",
            },
            Command::Kanban { kind } => match kind {
                KanbanKind::Create { .. } => "kanban create",
                KanbanKind::FromResearch { .. } => "kanban from-research",
                KanbanKind::List { .. } => "kanban list",
                KanbanKind::Move { .. } => "kanban move",
                KanbanKind::Update { .. } => "kanban update",
                KanbanKind::Show { .. } => "kanban show",
                KanbanKind::Delete { .. } => "kanban delete",
                KanbanKind::Prompt { .. } => "kanban prompt",
            },
            Command::Serve { .. } => "serve",
            Command::Rpc => "rpc",
        }
    }
}
