//! Askama templates for the HTMX dashboard (PRD §5.4).
//!
//! Compiled at build time from `templates/dashboard/*.html`; HTML escaping
//! is ON by default for `.html` templates (askama autoescape) — every
//! untrusted DB string flows through `{{ }}`, only the Rust-generated SVG
//! strings (already escaped in `svg.rs`) use `|safe`.

use askama::Template;

/// Full page layout: shared nav + `content` block.
#[derive(Template)]
#[template(path = "dashboard/base.html")]
pub struct BaseTemplate<'a> {
    pub title: &'a str,
    pub active: &'a str,
    pub content: &'a str,
}

/// Dashboard home: counts card (SSE), latest alerts, top ideas, charts.
#[derive(Template)]
#[template(path = "dashboard/home.html")]
pub struct HomeTemplate<'a> {
    pub counts_html: &'a str,
    pub alerts_html: &'a str,
    pub ideas_html: &'a str,
    pub views_chart: &'a str,
    pub seo_chart: &'a str,
}

/// The SSE fragment: health-card grid, swapped on each `counts` event.
/// `PartialEq` is the change detector: the stream compares a freshly-read
/// template against the last one sent and only emits on difference.
#[derive(Template, Debug, PartialEq, Eq, Clone)]
#[template(path = "dashboard/home_counts.html")]
pub struct CountsTemplate {
    pub videos: i64,
    pub channels: i64,
    pub scores: i64,
    pub ideas: i64,
    pub quota_used: u64,
    pub quota_limit: u64,
    pub integrity_ok: bool,
    pub integrity: String,
    /// "—" when no ingest has run (askama cannot Display `Option`).
    pub last_ingest: String,
    pub stale: i64,
    pub index_fresh: bool,
}

/// Video score table (title filter + top-N).
#[derive(Template)]
#[template(path = "dashboard/scores.html")]
pub struct ScoresTemplate<'a> {
    pub rows: &'a [ScoreRowView],
    pub q: &'a str,
    pub total: usize,
    pub limit: usize,
}

/// One row of the scores table (view-model; display strings precomputed).
pub struct ScoreRowView {
    pub video_id: String,
    pub title: String,
    pub channel: String,
    pub category: String,
    pub seo: String,
    pub geo: String,
    pub total: String,
    pub has_score: bool,
}

/// Row-expand fragment: the 17 component values (10 SEO + 7 GEO).
#[derive(Template)]
#[template(path = "dashboard/score_detail.html")]
pub struct ScoreDetailTemplate<'a> {
    pub video_id: &'a str,
    pub title: &'a str,
    pub seo_total: &'a str,
    pub geo_total: &'a str,
    pub total: &'a str,
    pub seo_components: &'a [(String, String)],
    pub geo_components: &'a [(String, String)],
    pub missing: bool,
}

/// Idea list page.
#[derive(Template)]
#[template(path = "dashboard/ideas.html")]
pub struct IdeasTemplate<'a> {
    pub rows: &'a [IdeaRowView],
}

/// One idea row fragment — the htmx `outerHTML` swap target after a status
/// POST. Shares the `idea_row` macro with the list page.
#[derive(Template)]
#[template(path = "dashboard/idea_row.html")]
pub struct IdeaRowTemplate<'a> {
    pub row: &'a IdeaRowView,
}

pub struct IdeaRowView {
    pub id: i64,
    pub title: String,
    pub score: String,
    pub status: String,
    pub source: String,
    pub created: String,
}

/// Keyword rank-trend table with position sparklines.
#[derive(Template)]
#[template(path = "dashboard/keywords.html")]
pub struct KeywordsTemplate<'a> {
    pub rows: &'a [KeywordTrendView],
    pub checked: usize,
}

pub struct KeywordTrendView {
    pub keyword: String,
    pub latest: String,
    pub previous: String,
    pub delta: String,
    pub delta_class: String,
    pub topics: String,
    pub spark: String,
}

/// Alerts page.
#[derive(Template)]
#[template(path = "dashboard/alerts.html")]
pub struct AlertsPageTemplate<'a> {
    pub list_html: &'a str,
    pub unread: usize,
    pub count: usize,
}

/// Alerts list fragment — replaced by htmx after mark-read / clear.
#[derive(Template)]
#[template(path = "dashboard/alerts_list.html")]
pub struct AlertsListTemplate<'a> {
    pub alerts: &'a [AlertRowView],
    pub unread: usize,
}

pub struct AlertRowView {
    pub id: i64,
    pub kind: String,
    pub severity: String,
    /// "—" when the alert has no channel.
    pub channel_id: String,
    pub message: String,
    pub created_at: String,
    pub read: bool,
}

/// Competitor scorecard page.
#[derive(Template)]
#[template(path = "dashboard/scorecard.html")]
pub struct ScorecardTemplate<'a> {
    pub rows: &'a [ScorecardRowView],
    pub median: ScorecardRowView,
    pub compared: usize,
}

pub struct ScorecardRowView {
    pub channel_id: String,
    pub title: String,
    pub videos: String,
    pub total_views: String,
    pub views_growth: String,
    pub avg_title_len: String,
    pub digit_ratio: String,
    pub howto_ratio: String,
    pub tag_overlap: String,
    pub centrality: String,
    pub seo_avg: String,
    pub seo_median: String,
    pub seo_min: String,
    pub seo_max: String,
    pub scored: String,
    pub is_median: bool,
}

/// Full health report page.
#[derive(Template)]
#[template(path = "dashboard/health.html")]
pub struct HealthTemplate<'a> {
    pub counts: &'a [(String, i64)],
    pub quota_used: &'a str,
    pub quota_limit: &'a str,
    /// "—" when no ledger date is recorded.
    pub quota_date: &'a str,
    pub integrity_ok: bool,
    pub integrity: &'a str,
    /// "—" when no ingest has run.
    pub last_ingest: &'a str,
    pub index_fresh: bool,
    /// "—" when the index was never built.
    pub index_last: &'a str,
    pub stale: &'a [(String, String, String)],
    pub stale_days: u32,
    pub engagement_complete: &'a str,
    pub disabled_videos: i64,
    pub disabled_view: i64,
    pub disabled_like: i64,
    pub disabled_comment: i64,
    pub privacy_unlisted: i64,
    pub privacy_private: i64,
}

/// Plain 404 page.
#[derive(Template)]
#[template(path = "dashboard/not_found.html")]
pub struct NotFoundTemplate<'a> {
    pub path: &'a str,
}
