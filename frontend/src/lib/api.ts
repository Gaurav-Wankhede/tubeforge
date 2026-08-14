import type {
  Counts,
  Trend,
  Alert,
  Video,
  VideoDetail,
  ScoreRow,
  ScoreDetail,
  Keyword,
  TrendingKeywordsResult,
  TagCloud,
  TagGap,
  VideoTags,
  CompetitorTags,
  ScorecardResponse,
  HealthReport,
  ChannelSnapshots,
  GapReport,
  OutlierVideo,
  CoverageTopic,
  Transcript,
  CommentItem,
  KeywordResearch,
  KeywordResearchHistory,
  ChannelAudit,
  PaginatedResponse,
  AnalysisOverview,
  AnalysisNextVideo,
  AnalysisKeywords,
  AnalysisTags,
} from './types'

const BASE = '/api'

async function get<T>(path: string, params?: Record<string, string>): Promise<T> {
  const url = new URL(path, window.location.origin)
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      if (v) url.searchParams.set(k, v)
    }
  }
  const res = await fetch(url.pathname + url.search)
  if (!res.ok) throw new Error(`API ${res.status}: ${await res.text()}`)
  return res.json()
}

async function post<T>(path: string, body?: unknown): Promise<T> {
  const res = await fetch(BASE + path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: body ? JSON.stringify(body) : undefined,
  })
  if (!res.ok) throw new Error(`API ${res.status}: ${await res.text()}`)
  if (res.status === 204) return undefined as T
  return res.json()
}

// Dashboard
export const api = {
  counts: () => get<Counts>(`${BASE}/counts`),
  trends: (period = '1m') => get<Trend[]>(`${BASE}/trends`, { period }),

  // Alerts
  alerts: () => get<Alert[]>(`${BASE}/alerts`),
  markAlertsRead: () => post<void>(`${BASE}/alerts/read`),
  clearAlerts: () => post<void>(`${BASE}/alerts/clear`),

  // Videos
  videos: (params?: { q?: string; page?: string; sort?: string }) =>
    get<PaginatedResponse<Video>>(`${BASE}/videos`, params),
  video: (id: string) => get<VideoDetail>(`${BASE}/videos/${id}`),

  // Scores
  scores: (params?: { q?: string; sort?: string }) =>
    get<ScoreRow[]>(`${BASE}/scores`, params),
  scoreDetail: (id: string) => get<ScoreDetail>(`${BASE}/scores/${id}`),

  // Channels
  channelSnapshots: (id: string) => get<ChannelSnapshots>(`${BASE}/channels/${id}/snapshots`),

  // Gap mining (Phase 6.5)
  gaps: () => get<GapReport>(`${BASE}/gaps`),
  gapOutliers: () => get<{ outliers: OutlierVideo[]; total: number }>(`${BASE}/gaps/outliers`),
  gapCoverage: () => get<{ topics: CoverageTopic[]; total: number }>(`${BASE}/gaps/coverage`),
  transcript: (id: string) => get<Transcript>(`${BASE}/transcripts/${id}`),
  comments: (id: string) => get<{ video_id: string; comments: CommentItem[]; total: number }>(`${BASE}/comments/${id}`),

  // Keywords
  keywords: () => get<Keyword[]>(`${BASE}/keywords`),
  trendingKeywords: () => get<TrendingKeywordsResult>(`${BASE}/keywords/trending`),
  inspectKeyword: (q: string, serp = 10) =>
    get<KeywordResearch>(`${BASE}/keywords/inspect`, { q, serp: String(serp) }),
  keywordHistory: (q: string) =>
    get<KeywordResearchHistory>(`${BASE}/keywords/history`, { q }),

  // Tags (Phase 1)
  tagCloud: () => get<TagCloud>(`${BASE}/tags`),
  tagGaps: () => get<TagGap[]>(`${BASE}/tags/gaps`),
  videoTags: (id: string) => get<VideoTags>(`${BASE}/tags/video/${id}`),
  competitorTags: (id: string) => get<CompetitorTags>(`${BASE}/tags/competitor/${id}`),

  // Scorecard
  scorecard: () => get<ScorecardResponse>(`${BASE}/scorecard`),
  audit: () => get<ChannelAudit[]>(`${BASE}/audit`),
  auditChannel: (id: string) => get<ChannelAudit>(`${BASE}/audit/${id}`),

  // Health
  health: () => get<HealthReport>(`${BASE}/health`),
  healthz: () => get<{ ok: boolean }>(`${BASE}/healthz`),

  // Analysis command center (computed, chart-ready — no raw records)
  analysisTopic: (topic: string, serp = 6) =>
    get<unknown>(`${BASE}/analysis/topic`, { q: topic, serp: String(serp) }),
  analysisOverview: () => get<AnalysisOverview>(`${BASE}/analysis/overview`),
  analysisNextVideo: () => get<AnalysisNextVideo>(`${BASE}/analysis/next-video`),
  analysisKeywords: (horizon = 7) =>
    get<AnalysisKeywords>(`${BASE}/analysis/keywords`, { horizon: String(horizon) }),
  analysisTags: () => get<AnalysisTags>(`${BASE}/analysis/tags`),
}
