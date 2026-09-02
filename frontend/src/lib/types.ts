export interface Counts {
  videos: number
  channels: number
  tags: number
  ideas: number
  alerts: number
  keywords: number
  kg_built: boolean
  kg_stats: KgStats
}

export interface Trend {
  date: string
  views: number
  subscribers: number
  videos: number
}

export interface Alert {
  id: number
  kind: string
  message: string
  severity: 'info' | 'warning' | 'critical'
  created_at: string
  read: boolean
}

export interface Video {
  video_id: string
  title: string
  channel_id: string
  channel_name: string
  published_at: string
  view_count: number
  like_count: number
  comment_count: number
  thumbnail_url: string
  tags: string[]
  seo_score: number | null
  geo_score: number | null
  total_score: number | null
}

export interface VideoDetail extends Video {
  description: string
  duration: number
  category_id: number
  freshness_score: number | null
  authority_score: number | null
  overall_score: number | null
  scores: ScoreBreakdown
}

export interface ScoreBreakdown {
  title_seo: number
  description_seo: number
  tags_relevance: number
  thumbnail_ctr: number
  engagement_rate: number
  freshness: number
  authority: number
  consistency: number
  growth_momentum: number
  keyword_density: number
  category_fit: number
  competitor_gap: number
  trend_alignment: number
  upload_frequency: number
  audience_retention: number
  cross_promotion: number
  monetization_potential: number
}

export interface ScoreRow {
  video_id: string
  title: string
  channel_id?: string
  channel_name: string
  overall_score: number
  freshness_score: number
  authority_score: number
  published_at: string
  views: number
  like_count?: number
  comment_count?: number
  duration_sec?: number
  thumb_url?: string
  outlier_multiplier?: number
}

export interface IdeaRationale {
  seo_total: number
  idea_fit: number
  competitor_gap: number
  engagement_boost: number
  centrality: number
  demand_matches: number
  keyword: string
  source_channel: string
}

export interface Idea {
  id: number
  title: string
  rationale: IdeaRationale
  score: number
  source_video?: string
}

export interface IdeasAnalysisResponse {
  ideas: Idea[]
  generated_at: string
  corpus_size: number
  note?: string
}

export interface Keyword {
  keyword: string
  rank: number
  trend: 'rising' | 'stable' | 'declining'
  sparkline: number[]
}

export interface TrendingKeyword {
  keyword: string
  score: number
  competition: number
  serp_mean_views: number
  volume_label: string
  actively_published: boolean
  source: 'google_trends' | 'youtube_search'
}

export interface TrendingKeywordsResult {
  trending: TrendingKeyword[]
  total: number
}

export interface TagCloud {
  tags: Array<{ name: string; count: number; trend: 'rising' | 'stable' | 'declining' }>
  total_unique: number
}

export interface TagGap {
  tag: string
  competitor_usage: number
  your_usage: number
  opportunity_score: number
}

export interface VideoTags {
  video_id: string
  title: string
  tags: Array<{ name: string; position: number; source: 'youtube' | 'extracted' | 'suggested' }>
}

export interface CompetitorTags {
  channel_id: string
  channel_name: string
  top_tags: Array<{ name: string; video_count: number; avg_views: number }>
  tag_diversity: number
}

export interface Scorecard {
  channel_id: string
  channel_name: string
  subscriber_count: number
  video_count: number
  avg_views: number
  avg_engagement: number
  overall_score: number
  is_own: boolean
}

export interface ScorecardResponse {
  rows: Scorecard[]
  own_channel: string | null
  own_flagged: boolean
}

export interface HeatmapPoint {
  start_time: number
  value: number
}

export interface PerformanceSignals {
  vph: number | null
  trending: boolean
  engagement_ratio: number | null
  engagement_score: number | null
  hook_retention: number | null
  mean_retention: number | null
  retention_score: number | null
  heatmap: HeatmapPoint[]
}

export interface ScoreDetail {
  video_id: string
  title: string
  seo_total: number
  geo_total: number
  total: number
  seo_components: Record<string, number>
  geo_components: Record<string, number>
  performance: PerformanceSignals | null
}

export interface ChannelSnapshot {
  at: string
  subscriber_count: number | null
  video_count: number | null
  total_views: number | null
}

export interface ChannelSnapshots {
  channel_id: string
  channel_name: string
  snapshots: ChannelSnapshot[]
}

export interface OutlierVideo {
  video_id: string
  title: string
  channel_id: string | null
  channel: string
  views: number
  channel_mean: number
  multiple: number
  channel_name: string
}

export interface CoverageTopic {
  topic: string
  videos: number
  channels: number
  mean_views: number
  newest_at: string | null
  no_short: boolean
  is_series: boolean
  score: number
  covering_channels: string[]
}

export interface GapReport {
  outliers: OutlierVideo[]
  topics: CoverageTopic[]
  freshness_gaps: string[]
  format_gaps: string[]
}

export interface Transcript {
  video_id: string
  title: string
  lang: string
  source: string
  words: number
  fetched_at: string
  text: string
}

export interface CommentItem {
  comment_id: string
  author: string
  text: string
  likes: number
  published_at: string
}

export interface SerpResult {
  video_id: string
  title: string
  channel: string
  channel_id: string
  view_count: number | null
  like_count: number | null
  comment_count: number | null
  upload_date: string | null
  tags: string[]
  seo_score: number
}

export interface CorpusMatch {
  video_id: string
  title: string
  channel: string | null
  view_count: number | null
  bm25: number
}

export interface TagSuggestion {
  tag: string
  usage: number
}

export interface RelatedKeyword {
  keyword: string
  popularity_rank: number
}

export interface KeywordResearch {
  keyword: string
  serp_total: number
  serp_mean_views: number
  serp_total_views: number
  volume_label: string
  ranking_channels: number
  competition_score: number
  opportunity_score: number
  keyword_score: number
  verdict: string
  suggested_tags: TagSuggestion[]
  related_keywords: RelatedKeyword[]
  recent_uploads: number
  actively_published: boolean
  corpus_resonance: number | null
  corpus_matches: CorpusMatch[]
  serp: SerpResult[]
}

export interface ResearchSnapshot {
  at: string
  volume_label: string
  serp_total: number
  serp_mean_views: number
  ranking_channels: number
  competition_score: number
  opportunity_score: number
  actively_published: boolean
  suggested_tags: string[]
  related_keywords: string[]
}

export interface KeywordResearchHistory {
  keyword: string
  snapshots: ResearchSnapshot[]
  total: number
}

export interface AuditComponent {
  name: string
  score: number
  weight: number
  detail: string
}

export interface ChannelAudit {
  channel_id: string
  channel_name: string
  total_score: number
  grade: string
  verdict: string
  components: AuditComponent[]
}

export interface HealthReport {
  counts: {
    channels: number
    videos: number
    scores: number
    keywords: number
    keyword_rankings: number
    ideas: number
    alerts: number
    edges: number
    ingest_log: number
  }
  privacy: { unlisted: number; private: number }
  last_ingest: { at: string; batch_id: string; item: string; status: string } | null
  quota: { videos_list_used: number; daily_limit: number; date: string }
  integrity: string
  stale_channels: Array<{ channel_id: string; title: string; fetched_at: string }>
  stale_days: number
  index: { last_reindex_at: string | null; fresh: boolean }
  metadata_completeness: {
    engagement_complete: number
    disabled_metrics: { videos: number; view_count: number; like_count: number; comment_count: number }
  }
}

export interface PaginatedResponse<T> {
  items: T[]
  total: number
  page: number
  page_size: number
}

// ---- Analysis command center (computed, chart-ready — no raw records) ----

export interface ChartPoint {
  label: string
  value: number
}

export interface TagGapInsight {
  tag: string
  competitor_usage: number
  our_usage: number
  opportunity: number
}

export interface CompetitorMedians {
  subscriber_median: number
  avg_views_median: number
  score_median: number
  channel_count: number
}

export interface AnalysisOverview {
  channel_name: string
  subscriber_count: number
  video_count: number
  total_views: number
  avg_views: number
  avg_score: number
  best_video_title: string
  growth: ChartPoint[]
  competitor: CompetitorMedians
  tag_gaps: TagGapInsight[]
}

export interface NextVideoRecommendation {
  topic: string
  verdict: string
  next_opportunity: number
  opportunity_score: number
  competition_score: number
  volume_label: string
  title: string
  description: string
  tags: string[]
  reliability: string
  /** VidIQ-style View Prediction tier. */
  prediction: string
  /** Plain-language "make THIS because...". */
  why: string
}

export interface AnalysisNextVideo {
  /** Ranked list of candidate next-videos (top N). */
  recommendations: NextVideoRecommendation[]
  /** Newest stored research snapshot time — surfaces staleness. */
  research_at: string | null
}

export interface KeywordOpportunity {
  keyword: string
  opportunity: number
  competition: number
  volume: string
  verdict: string
  trend: ChartPoint[]
}

export interface AnalysisKeywords {
  opportunities: KeywordOpportunity[]
  horizon_days: number
  research_at: string | null
}

export interface AnalysisTags {
  tag_gaps: TagGapInsight[]
}

// ---- Knowledge Graph types (internal enhancement — no separate API) ----

/** Graph-aware scores for a video (from `graph_scores` field on score detail). */
export interface GraphScores {
  tag_authority: number
  topic_dominance: number
  keyword_competition: number
}

/** A graph-based content gap (from `graph_gaps` field on gaps response). */
export interface GraphGap {
  topic: string
  topic_id: string
  opportunity_score: number
}

/** A graph-based video idea (from `graph_ideas` field on ideas response). */
export interface GraphIdea {
  title: string
  score: number
  rationale: string
  source: 'knowledge_graph'
}

/** KG build status (from `kg_built` and `kg_stats` fields on counts response). */
export interface KgStats {
  entities: number
  relations: number
  communities: number
}

/** Enhanced score detail with optional graph_scores. */
export interface ScoreDetailWithGraph extends ScoreDetail {
  graph_scores: GraphScores | null
}

/** Enhanced gap report with optional graph_gaps. */
export interface GapReportWithGraph extends GapReport {
  graph_gaps: GraphGap[] | null
}

/** Enhanced ideas response with optional graph_ideas. */
export interface IdeasAnalysisWithGraph extends IdeasAnalysisResponse {
  graph_ideas: GraphIdea[] | null
}

/** Enhanced scorecard row with optional centrality. */
export interface ScorecardWithCentrality {
  channel_id: string
  channel_name: string
  subscriber_count: number
  video_count: number
  avg_views: number
  avg_engagement: number
  overall_score: number
  centrality: number | null
  is_own: boolean
}

/** Enhanced tag gap with optional authority. */
export interface TagGapWithAuthority {
  tag: string
  competitor_usage: number
  your_usage: number
  opportunity_score: number
  tag_authority: number | null
}

// ---- Kanban types (Phase 7 Production Board) ----

export interface KanbanTicket {
  ticket_id: string
  title: string
  channel: string
  status: 'todo' | 'inprogress' | 'done' | 'published'
  topic: string | null
  framework: string | null
  optimal_duration_sec: number | null
  target_keyword: string | null
  youtube_url: string | null
  video_id: string | null
  research_ref: string | null
  notes: string | null
  created_at: string
  updated_at: string
}

export interface KanbanSummary {
  total: number
  todo: number
  inprogress: number
  done: number
  published: number
}

export interface KanbanListResponse {
  summary: KanbanSummary
  tickets: KanbanTicket[]
}

export interface KanbanPromptResponse {
  ticket_id: string
  title: string
  channel: string
  prompt: string
}

