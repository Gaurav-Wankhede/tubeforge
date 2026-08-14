# Frontend → Backend Connection Map

**Project:** TubeForge — Local-first YouTube SEO/GEO growth engine
**Date:** August 14, 2026 | **Status:** Complete mapping (refreshed for the engine-independence stack — raw-Hyper + WebSocket JSON-RPC + SSE + `tfdb`; stdio `rpc` bridge added)

---

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                        FRONTEND (React SPA)                          │
│  /Users/gauravwankhede/Projects/tubeforge/frontend/src/             │
│                                                                      │
│  Routes (pages)        Components           API Client (lib/api.ts)  │
│  ─────────────         ──────────           ───────────────────────  │
│  Dashboard.tsx         Layout.tsx           api.counts()             │
│  Scores.tsx            ConnectionStatus.tsx  api.scores()             │
│  ScoreDetail.tsx       FreshnessBadge.tsx    api.scoreDetail()        │
│  Ideas.tsx                                 api.ideasAnalyze()        │
│  Keywords.tsx                              api.keywords()             │
│  KeywordOpportunity.tsx                     api.inspectKeyword()      │
│  Scorecard.tsx                             api.scorecard()           │
│  Alerts.tsx                                api.alerts()              │
│  Health.tsx                                api.health()              │
│  Gaps.tsx                                  api.gaps()                │
│  Tags.tsx                                  api.tagCloud()            │
│  Videos.tsx                                api.videos()              │
│  Audit.tsx                                 api.audit()               │
│  TopicResearch.tsx                         api.analysisTopic()       │
│  NextVideo.tsx                             api.analysisNextVideo()   │
│  TagIntelligence.tsx                        api.tagsGaps()            │
│  AnalysisDashboard.tsx                      api.analysisOverview()    │
└──────────────────────────────┬──────────────────────────────────────┘
                               │ HTTP (fetch)
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│                     BACKEND (raw-Hyper server)                         │
│  /Users/gauravwankhede/Projects/tubeforge/src/serve.rs               │
│                                                                      │
│  API Routes (/api/*)        HTMX Pages (/legacy/* or /)             │
│  ──────────────────         ──────────────────────────              │
│  GET  /api/counts           GET  / (home/dashboard)                 │
│  GET  /api/trends           GET  /scores                            │
│  GET  /api/alerts           GET  /scores/{id}                       │
│  POST /api/alerts/read      GET  /ideas                             │
│  POST /api/alerts/clear     POST /ideas/{id}/{status}               │
│  GET  /api/scores           GET  /alerts                            │
│  GET  /api/scores/{id}      POST /alerts/read                       │
│  GET  /api/videos           POST /alerts/clear                      │
│  GET  /api/videos/{id}      GET  /keywords                          │
│  GET  /api/ideas/analyze    GET  /scorecard                         │
│  GET  /api/keywords         GET  /health                            │
│  GET  /api/keywords/trending                                       │
│  GET  /api/keywords/inspect                                        │
│  GET  /api/keywords/history                                        │
│  GET  /api/scorecard                                                │
│  GET  /api/audit                                                    │
│  GET  /api/audit/{id}                                               │
│  GET  /api/health                                                   │
│  GET  /api/gaps                                                     │
│  GET  /api/gaps/outliers                                            │
│  GET  /api/gaps/coverage                                            │
│  GET  /api/tags                                                     │
│  GET  /api/tags/gaps                                                │
│  GET  /api/tags/video/{id}                                          │
│  GET  /api/tags/competitor/{id}                                     │
│  GET  /api/transcripts                                              │
│  GET  /api/transcripts/{id}                                         │
│  GET  /api/comments/{id}                                            │
│  GET  /api/analysis/*                                               │
│  GET  /events (SSE)                                                 │
│  GET  /healthz                                                      │
│  WS   /ws (RPC)                                                     │
└──────────────────────────────┬──────────────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│                        DOMAIN LAYER                                  │
│                                                                      │
│  analytics/                  storage/               fetch/           │
│  ──────────                  ────────               ──────           │
│  graph.rs                    tfdb/ (engine)         rss.rs           │
│  ideas.rs                    db.rs (repository)     oembed.rs        │
│  keywords.rs                                         api.rs           │
│  reports.rs                                          quota.rs         │
│  growth.rs forecast.rs                              ytdlp.rs         │
│  scoring/                                                            │
│  ─────────                                                            │
│  seo.rs                                                              │
│  geo.rs                                                              │
│  psych.rs recommend.rs weights.rs                                   │
│  ─────────────────────────────────────────────────────────────────   │
│  KG MODULES (integrated, internal-only):                             │
│  kg.rs, kg_algorithms.rs, kg_builder.rs, graph_aware.rs             │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Complete Route → Handler → Domain → Storage Mapping

### Dashboard & Health

| Frontend Route | API Endpoint | Handler | Domain Module | Storage Method |
|---|---|---|---|---|
| Dashboard.tsx | `GET /api/counts` | `counts_api` | `reports::health` | `db.count()` |
| Dashboard.tsx | `GET /events` (SSE) | `events` | `reports::health` | `db.list_alerts()`, `db.list_ideas()` |
| Dashboard.tsx | `GET /api/trends` | `trends_api` | `keywords::trend_rows` | `db.list_rankings()` |
| Health.tsx | `GET /api/health` | `health_api` | `reports::health` | `db.count()`, `db.last_ingest()` |

### Scores

| Frontend Route | API Endpoint | Handler | Domain Module | Storage Method |
|---|---|---|---|---|
| Scores.tsx | `GET /api/scores` | `scores_api` | — | `db.all_videos()`, `db.all_scores()` |
| ScoreDetail.tsx | `GET /api/scores/{id}` | `score_detail_api` | — | `db.get_score()`, `db.get_video()` |

### Ideas

| Frontend Route | API Endpoint | Handler | Domain Module | Storage Method |
|---|---|---|---|---|
| Ideas.tsx | `GET /api/ideas/analyze` | `ideas_analyze_api` | `ideas::generate` | `db.list_keywords()`, `db.all_scores()` |
| Ideas.tsx | `POST /ideas/{id}/{status}` | `ideas_status` | — | `db.set_idea_statuses()` |

### Keywords

| Frontend Route | API Endpoint | Handler | Domain Module | Storage Method |
|---|---|---|---|---|
| Keywords.tsx | `GET /api/keywords` | `keywords_api` | `keywords::trend_rows` | `db.list_rankings()` |
| KeywordOpportunity.tsx | `GET /api/keywords/inspect` | `keywords_inspect_api` | `research::inspect` | `db.all_videos()` |
| KeywordOpportunity.tsx | `GET /api/keywords/trending` | `keywords_trending_api` | `keywords::trend_rows` | `db.list_rankings()` |
| KeywordOpportunity.tsx | `GET /api/keywords/history` | `keywords_history_api` | — | `db.keyword_research_history()` |

### Scorecard & Audit

| Frontend Route | API Endpoint | Handler | Domain Module | Storage Method |
|---|---|---|---|---|
| Scorecard.tsx | `GET /api/scorecard` | `scorecard_api` | `reports::scorecard` | `db.all_videos()`, `db.all_scores()`, `db.list_edges()` |
| Audit.tsx | `GET /api/audit` | `audit_api` | `reports::audit` | `db.all_channels()`, `db.list_competitors()` |
| Audit.tsx | `GET /api/audit/{id}` | `audit_channel_api` | `reports::audit` | `db.get_channel()` |

### Alerts

| Frontend Route | API Endpoint | Handler | Domain Module | Storage Method |
|---|---|---|---|---|
| Alerts.tsx | `GET /api/alerts` | `alerts_api` | — | `db.list_alerts()` |
| Alerts.tsx | `POST /api/alerts/read` | `alerts_read_api` | — | `db.mark_alerts_read()` |
| Alerts.tsx | `POST /api/alerts/clear` | `alerts_clear_api` | — | `db.clear_alerts()` |

### Gaps & Tags

| Frontend Route | API Endpoint | Handler | Domain Module | Storage Method |
|---|---|---|---|---|
| Gaps.tsx | `GET /api/gaps` | `gaps_api` | `gaps::report` | `db.all_videos()`, `db.all_channels()` |
| Gaps.tsx | `GET /api/gaps/outliers` | `gaps_outliers_api` | `gaps::outliers` | `db.all_videos()`, `db.all_channels()` |
| Gaps.tsx | `GET /api/gaps/coverage` | `gaps_coverage_api` | `gaps::coverage` | `db.all_videos()` |
| Tags.tsx | `GET /api/tags` | `tags_cloud_api` | `tags::tag_cloud` | `db.tag_cloud()` |
| Tags.tsx | `GET /api/tags/gaps` | `tags_gaps_api` | `tags::tag_gaps` | `db.tag_gaps()` |
| TagIntelligence.tsx | `GET /api/tags/video/{id}` | `video_tags_api` | — | `db.get_video_tags()` |
| TagIntelligence.tsx | `GET /api/tags/competitor/{id}` | `competitor_tags_api` | — | `db.get_competitor_tag_stats()` |

### Videos & Transcripts

| Frontend Route | API Endpoint | Handler | Domain Module | Storage Method |
|---|---|---|---|---|
| Videos.tsx | `GET /api/videos` | `videos_api` | — | `db.all_videos()` |
| Videos.tsx | `GET /api/videos/{id}` | `video_detail_api` | — | `db.get_video()` |
| — | `GET /api/transcripts/{id}` | `transcript_api` | — | `db.get_transcript()` |
| — | `GET /api/comments/{id}` | `comments_api` | — | `db.list_comments()` |

### Analysis (Command Center)

| Frontend Route | API Endpoint | Handler | Domain Module | Storage Method |
|---|---|---|---|---|
| AnalysisDashboard.tsx | `GET /api/analysis/overview` | `analysis_overview_api` | `reports::health` | `db.count()` |
| AnalysisDashboard.tsx | `GET /api/analysis/keywords` | `analysis_keywords_api` | `forecast` | `db.keyword_research_all()` |
| NextVideo.tsx | `GET /api/analysis/next-video` | `analysis_next_video_api` | `ideas::generate` | `db.list_keywords()`, `db.all_scores()` |
| TopicResearch.tsx | `GET /api/analysis/topic` | `analysis_topic_api` | `research::discover` | `db.all_videos()`, `db.register_competitors()` |

---

## Knowledge Graph Integration Points (Internal-Only — No Separate API)

> **Architecture Decision:** The Knowledge Graph is an **internal enhancement** to existing APIs. There are **NO** `/api/kg/*` endpoints. All KG processing happens internally within existing handlers. KG signals are returned as additional optional fields on existing endpoints.

### Enhanced Existing Endpoints (KG-Aware)

| Existing Endpoint | Enhancement | KG Signal Added | KG Module |
|---|---|---|---|
| `GET /api/scores/{id}` | Add `graph_scores` field | `tag_authority`, `topic_dominance`, `keyword_competition` | `graph_aware::compute_graph_scores` |
| `GET /api/scorecard` | Add `centrality` ranking | PageRank centrality per channel | `kg_algorithms::pagerank` + `graph_aware` |
| `GET /api/gaps` | Add `graph_gaps` field | Community coverage analysis | `graph_aware::find_content_gaps` |
| `GET /api/keywords/inspect` | Add `related_entities` | Entity graph from KG | `kg_retriever::retrieve` |
| `GET /api/tags/gaps` | Add `tag_authority` | Tag authority scores from KG | `graph_aware::compute_tag_authority` |
| `GET /api/ideas/analyze` | Add `graph_ideas` field | Community gap detection | `graph_aware::generate_graph_ideas` |
| `GET /api/analysis/*` | Add graph signals | Community, central channels, content gaps | `graph_aware` |
| `GET /api/counts` + SSE | Add `kg_built`, `kg_stats` | KG build status + entity/relation counts | `kg_builder::load_or_build` |
| `GET /api/audit` | Add channel centrality | PageRank centrality | `kg_algorithms::pagerank` |
| `GET /api/audit/{id}` | Add topic dominance | Channel's share of topic cluster | `graph_aware::compute_topic_dominance` |

### Explicitly Excluded Endpoints (By Design — YAGNI)

The following endpoints are **NOT built** — their functionality is available through enhanced existing endpoints or CLI:

| Excluded Endpoint | Why | Alternative |
|---|---|---|
| `GET /api/kg/graph` | No separate KG API | Graph SVG via enhanced `GET /api/analysis/graph` |
| `GET /api/kg/graph/{id}` | No separate KG API | Local graph via query params on existing analysis endpoint |
| `POST /api/kg/build` | No HTTP build trigger | KG lazy-builds on first KG-dependent query (`kg_builder::load_or_build`) |
| `GET /api/kg/status` | No separate KG API | KG stats in `GET /api/counts` response |
| `POST /api/kg/query` | No separate KG API | KG queries via enhanced existing endpoints (no CLI command) |
| `GET /api/kg/entity/{id}` | No separate KG API | Entity data via enhanced existing endpoints |
| `GET /api/kg/communities` | No separate KG API | Communities via enhanced `GET /api/analysis/*` |

---

## CLI Commands → Backend Mapping

| CLI Command | Command File | Domain Module | Storage Method |
|---|---|---|---|
| `tubeforge init` | `commands/init.rs` | — | `Db::open()` |
| `tubeforge ingest channels` | `commands/ingest.rs` | `ingest` | `Batch::upsert_channel()`, `Batch::upsert_video()` |
| `tubeforge ingest links` | `commands/ingest.rs` | `ingest` | `Batch::upsert_video()` |
| `tubeforge refresh` | `commands/refresh.rs` | `ingest` | `Batch::upsert_video()` |
| `tubeforge score` | `commands/score.rs` | `scoring` | `db.upsert_score()` |
| `tubeforge ideas` | `commands/ideas.rs` | `ideas::generate` | `db.upsert_idea()` |
| `tubeforge keywords add` | `commands/keywords.rs` | — | `db.add_keywords()` |
| `tubeforge keywords check` | `commands/keywords.rs` | `keywords` | `db.upsert_ranking()` |
| `tubeforge scorecard` | `commands/scorecard.rs` | `reports::scorecard` | `db.all_videos()`, `db.all_scores()` |
| `tubeforge health` | `commands/health.rs` | `reports::health` | `db.count()` |
| `tubeforge alerts` | `commands/alerts.rs` | `reports::alerts` | `db.list_alerts()` |
| `tubeforge gaps` | `commands/gaps.rs` | `gaps::report` | `db.all_videos()` |
| `tubeforge tags` | `commands/tags.rs` | `tags::tag_cloud` | `db.tag_cloud()` |
| `tubeforge rpc` | `commands/rpc.rs` | `serve::rpc::dispatch` | `Db` (stdio JSON-RPC for agents) |

> **KG has no dedicated CLI or HTTP surface** (YAGNI, PRD v3.15/v4.0). It is
> **lazy-loaded** (`kg_builder::load_or_build`) on first KG-dependent endpoint
> and cached for the server lifetime; `graph_scores`, `centrality`, graph
> ideas/gaps are returned as optional fields on existing endpoints. There is
> no `tubeforge kg build` / `kg query` / `kg status`.

---

## Identified Issues & Gaps

> **Status (Aug 14, 2026):** the KG is now **integrated** — `graph_scores`,
> `centrality`, graph ideas/gaps are returned by the existing endpoints via
> `graph_aware`/`kg_builder` (lazy-loaded). Issues 1 and 3 below are resolved
> at the backend; the remaining items are frontend polish + the deferred
> incremental-KG-on-ingest. `kg_retriever.rs` / `graph_viz.rs` were **not
> built** (YAGNI) — hybrid vector retrieval is deferred, and graph SVG is
> served through existing analysis endpoints.

### Issue 1: Existing Endpoints Need KG Enhancement
**Severity:** High  
**Description:** KG modules are built but existing endpoints don't yet return KG signals.  
**Fix:** Enhance `score_detail_api`, `scorecard_api`, `gaps_api`, `keywords_inspect_api`, `tags_gaps_api`, `ideas_analyze_api` to call internal KG modules and include results as additional optional fields.

### Issue 2: No Graph Visualization Frontend
**Severity:** Medium  
**Description:** `graph_viz.rs` produces SVG but no frontend component displays it.  
**Fix:** Embed graph SVG within existing AnalysisDashboard page (via enhanced `GET /api/analysis/graph`). No new route needed.

### Issue 3: Scores Page Doesn't Show Graph Components
**Severity:** Medium  
**Description:** The 3 new graph-based SEO components (`tag_authority`, `topic_dominance`, `keyword_competition`) are computed but not displayed in the score detail.  
**Fix:** Update `score_detail_api` to include `graph_scores` field when KG is available; frontend renders breakdown.

### Issue 4: SSE Stream Doesn't Include KG Metrics
**Severity:** Low  
**Description:** The SSE `counts` event doesn't include KG stats (entity count, relation count, community count).  
**Fix:** Extend `CountsTemplate` with `kg_built`, `kg_entity_count`, `kg_relation_count` fields when KG is available.

### Issue 5: No Incremental KG Update on Ingest
**Severity:** Medium  
**Description:** When new videos are ingested, the KG is not updated automatically.  
**Fix:** Call `kg_builder::build(db, Incremental)` after ingest completes (internal function, no API change).

### Issue 6: Frontend Types Missing KG Types
**Severity:** Medium  
**Description:** `frontend/src/lib/types.ts` has no types for graph scores, graph context, or KG stats.  
**Fix:** Add `GraphScores`, `GraphContext`, `KgStats` types to frontend `types.ts`. Keep optional for backward compatibility.

### Issue 7: No Error Handling for Missing KG
**Severity:** Low  
**Description:** When KG is not built, enhanced fields should gracefully return null/undefined.  
**Fix:** KG-enhanced fields return `null` when KG not built; frontend hides sections with null data.

---

## Connection Flow Diagram

```
Frontend Component
       │
       ▼
frontend/src/lib/api.ts
       │  fetch('/api/...')
       ▼
serve.rs → api.rs (Router)
       │
       ▼
Handler Function (e.g., scores_api)
       │
       ├──► reports:: (analytics) ──► db.* (storage, tfdb)
       │
       ├──► kg_builder:: (lazy) ──► kg_entities, kg_relations, kg_communities
       │
       └──► graph_aware:: ──► graph_scores / centrality / gaps (SVG via existing analysis)
```

---

## Priority Fix List

| # | Issue | Priority | Effort |
|---|---|---|---|
| 1 | Enhance existing endpoints with KG signals (internal KG calls) | High | 3h |
| 2 | Add KG types to frontend `types.ts` (GraphScores, GraphContext, KgStats) | High | 1h |
| 3 | Embed graph SVG in existing AnalysisDashboard page | High | 2h |
| 4 | Update score detail frontend to render graph components | Medium | 1h |
| 5 | Add incremental KG update on ingest (internal function call) | Medium | 1h |
| 6 | Extend SSE counts with KG stats (`kg_built`, `kg_entity_count`) | Low | 0.5h |
| 7 | Add graceful null handling for missing KG in frontend | Low | 0.5h |

**Total estimated effort:** ~9 hours (down from ~12h in previous plan — YAGNI savings from not building separate endpoints).
