# TubeForge — Creator Growth Plan

**Goal:** Turn TubeForge into a fully local, free VidIQ / TubeBuddy replacement for the owner channel, using the database as the long-term memory for analytics, recommendations, and trend tracking.

**Owner channel (default seed):**
- Handle: `@GauravWankhede-TECHVERSE`
- Alternative handle: `@gauravwankhede-techverse`
- Channel ID: `UC4BK6cXh5id7rG_k-rUqQTA`

**Status:** Draft — aligns with `PRD.md` v4.0 and `LLD.md` v1.6. The yt-dlp data-source option is now incorporated in the PRD (§1/§4/§5.10) and LLD (§5); this plan tracks the owner-channel growth workstream.

---

## Guiding principles

1. **No subscriptions, no paid APIs.** Everything must be runnable locally for free.
2. **Database-first.** Every raw signal, score, and recommendation is persisted so future analysis can compare against history.
3. **Public-data only.** No authentication, no cookies, no private/unlisted/age-restricted content access.
4. **Async, bounded concurrency.** "Bulk" means many concurrent single-item calls, not one giant request.
5. **Opt-in enrichment.** New fetch sources are optional; the existing RSS / oEmbed / API path stays the default.

---

## Data-source policy

| Source | Always available | Requires key | Notes |
|---|---|---|---|
| YouTube Channel RSS | yes | no | Primary channel/video list source. |
| YouTube oEmbed | yes | no | Basic public metadata for single videos. |
| YouTube Data API v3 | optional | yes | Rich metadata when user supplies `YOUTUBE_API_KEY`. |
| **yt-dlp (InnerTube client)** | **optional** | **no** | **New (Phase 6.5, verified Aug 2026).** Keyless public metadata (incl. heatmap/retention curve + channel_follower_count), transcripts, comments, search, channel enumeration. Not HTML scraping — yt-dlp calls YouTube's private InnerTube JSON API (mobile/web clients). Bounded concurrency, no cookies, no auth. |

> **PRD/LLD amendment required:** `PRD.md` §1 and §4 currently state "No page scraping of any kind." yt-dlp is NOT page scraping (it uses YouTube's InnerTube JSON API), but the amendment should still note the optional keyless enrichment path, no auth, rate-limited, user-controlled.

---

## Daily competitor refresh workflow (API-first)

Based on the official YouTube Data API quota table, the free plan gives:
- **10,000 units/day** for most `list` endpoints.
- **100 `search.list` calls/day** only.
- **`videos.list`**: **1 unit per call**, up to **50 video IDs per call**.
- **`channels.list`**: **1 unit per call**.
- **`playlistItems.list`**: **1 unit per call**, up to **50 items per page**.

This means you can refresh a large competitor set every day without touching the expensive `search.list` bucket, as long as you already know competitor channel IDs/handles.

### Recommended daily job

For every tracked competitor channel:

1. **`channels.list`** — `part=contentDetails,snippet,statistics,topicDetails`  
   Get the channel’s uploads playlist ID (`contentDetails.relatedPlaylists.uploads`) plus public metadata and rounded stats.  
   Cost: **1 unit per competitor**.

2. **`playlistItems.list`** — `playlistId=<uploads>`, `part=snippet,contentDetails`, `maxResults=50`  
   Walk the uploads playlist (1 page = 50 most-recent videos). For deeper history, request additional pages.  
   Cost: **1 unit per 50 videos**.

3. **`videos.list`** — `id=...,...,` (batched 50 at a time), `part=snippet,contentDetails,statistics,topicDetails`  
   Fetch rich metadata + public stats for every collected video ID.  
   Cost: **1 unit per 50 videos**.

### Quota budget example

| Competitors | Recent videos each | `channels.list` | `playlistItems.list` | `videos.list` | **Total units/day** |
|---|---:|---:|---:|---:|---:|
| 10 | 50 | 10 | 10 | 10 | **30** |
| 25 | 50 | 25 | 25 | 25 | **75** |
| 50 | 50 | 50 | 50 | 50 | **150** |
| 100 | 50 | 100 | 100 | 100 | **300** |

Even 100 competitors refreshing 50 videos each costs only **~300 units/day** — well under the 10,000-unit daily cap.

### Optimizations to stay within quota

- **Never use `search.list` for routine refreshes.** Reserve the 100 daily `search.list` calls only for discovering new competitor channels or keyword research.
- **Batch IDs aggressively.** Always call `videos.list` with 50 IDs per request.
- **Use ETags / conditional requests.** Store the returned ETag and send `If-None-Match` on the next refresh; unchanged resources return `304 Not Modified` and cost only the request unit but no data transfer.
- **Use `fields` parameter.** Only request the nested fields actually stored in TubeForge (e.g., `items(id,snippet(channelId,title,description,tags),statistics,topicDetails)`).
- **Skip unchanged videos.** If `publishedAt` and stats have not changed, avoid re-processing heavy text fields.

### Important API limitations

- **`subscriberCount` is rounded.** YouTube deliberately rounds subscriber counts in `channels.list` responses. Exact subscriber numbers are no longer available via the public API.
- **Like counts can be hidden** by the video owner; treat them as optional.
- **Comment counts are public when comments are enabled**, but fetching actual comment text requires `commentThreads.list` (also 1 unit per call) and can burn quota quickly.

### How this feeds the VidIQ/TubeBuddy analysis

The daily refresh populates the same database tables used by the rest of the plan:
- **Tags Analyzer** (Phase 1) reads competitor `tags`.
- **Keyword Research** (Phase 2) scores keywords against the refreshed corpus.
- **Title/Description Optimizer** (Phase 3) benchmarks against competitor titles.
- **Channel Audit** (Phase 4) computes engagement trends from historical snapshots.
- **Next-Topic Recommender** (Phase 5) identifies content gaps between your channel and refreshed competitors.

### Implementation notes

- Add a `competitors` table with `channel_id`, `handle`, `added_at`, `last_refreshed_at`.
- Add a scheduled/CLI command: `tubeforge refresh --competitors --max-videos-per-channel 50`.
- Persist quota usage per call in the existing `quota` ledger so the dashboard can show remaining budget.

---

## Phase 0 — Foundation (in progress)

**Objective:** Make the existing codebase green and stable before adding growth features.

- [x] Fix deduplication compile errors in `src/ingest.rs`, `src/storage/db.rs`, `src/analytics/keywords.rs`.
- [x] Restore `keywords::report`, `extract_video_ids`, `parse_input_items`, `parse_links_input`, `valid_*_checksum`, `InputItem`, `ChannelRef` exports used by `src/commands/ingest.rs`.
- [x] Fix `Batch::merge_channel` parameter cloning issue.
- [x] Add `IngestSummary.rejected` field.
- [ ] Run full `cargo test --release` and verify migration 004 on a live DB.
- [ ] Restart `tubeforge serve` and smoke-test dashboard.

**Exit gate:** `cargo test --release` passes, clippy clean, dashboard loads.

---

## Phase 1 — Tags Analyzer

**Objective:** Deliver VidIQ/TubeBuddy-style tag intelligence.

### 1.1 Database additions
- `tag_frequency(channel_id?, competitor?)` — most-used tags across own / competitor videos.
- `tag_overlap(video_id_a, video_id_b)` — Jaccard overlap between two tag sets.
- `tag_opportunity(keyword)` — how often a tag appears in competitor videos but not own videos.

### 1.2 Module
- `src/analytics/tags.rs`
  - `analyze_channel(db, channel_id)` → tag cloud + distribution.
  - `suggest_tags(db, video_id)` → missing tags ranked by competitor frequency and SEO weight.
  - `compare_videos(db, a, b)` → overlap report.

### 1.3 CLI
- `tubeforge tags analyze --channel <id>`
- `tubeforge tags suggest --video <id>`
- `tubeforge tags compare <id_a> <id_b>`

### 1.4 Dashboard
- New `/tags` page with:
  - Own top tags table.
  - Competitor tag gap list.
  - Per-video tag suggestions.

**Exit gate:** Tag analysis produces actionable suggestions for the owner channel.

---

## Phase 2 — Keyword Research

**Objective:** Free keyword discovery and opportunity scoring.

### 2.1 Fetch layer
- `src/fetch/keywords.rs`
  - Query YouTube search autocomplete (`https://suggestqueries.google.com/complete/search?client=youtube&...`).
  - Parse suggestions into `KeywordSuggestion` structs.
  - Cache results in `keyword_suggestions` table (query + suggestions + fetched_at).

### 2.2 Scoring
- `KeywordOpportunity` score combining:
  - Search volume proxy (suggestion position / related queries count).
  - Competition proxy (BM25 result count in own + competitor corpus).
  - Relevance to channel niche (tag/channel title overlap).

### 2.3 CLI
- `tubeforge keywords research <query>`
- `tubeforge keywords opportunity <query>`

### 2.4 Dashboard
- `/keywords/research` page with autocomplete results and opportunity scores.

**Exit gate:** User can enter a seed term and get a prioritized list of related keywords.

---

## Phase 3 — Title / Description Optimizer

**Objective:** Score and improve titles/descriptions before upload.

### 3.1 Scoring signals
- Length (title 40–70 chars, description ≥ 150 chars).
- Keyword presence in first 60 chars of title.
- Power words / question hooks.
- Tag inclusion in description.
- Emoji / formatting balance.
- Competitor title pattern match.

### 3.2 Module
- `src/analytics/optimizer.rs`
  - `score_title(title, tags, keywords)` → score + breakdown.
  - `score_description(description, tags, keywords)` → score + breakdown.
  - `suggest_title_variants(title, keywords)` → 3–5 variants.

### 3.3 CLI
- `tubeforge optimize title "..." --tags ... --keywords ...`
- `tubeforge optimize description "..." --tags ... --keywords ...`

### 3.4 Dashboard
- `/optimizer` page with a form to paste draft title/description and get live scores + variants.

**Exit gate:** Draft title/description gets a 0–100 score with concrete improvement hints.

---

## Phase 4 — Channel Audit / Scorecard

**Objective:** One-page health and growth report for the owner channel.

### 4.1 Metrics
- Consistency score (upload frequency over last 30/90 days).
- SEO score (average video SEO score).
- Engagement trend (views/likes/comments per video over time).
- Tag coverage (% of videos with ≥ N tags).
- Thumbnail presence (% of videos with custom thumbnails — inferred from stored thumb URLs).
- Keyword rank trend (latest vs previous snapshots).
- Competitor gap (top 5 tags/keywords competitors use more).

### 4.2 Module
- `src/analytics/audit.rs` → `channel_audit(db, channel_id)`.

### 4.3 CLI
- `tubeforge audit --channel <id>`

### 4.4 Dashboard
- `/scorecard` page becomes the main channel audit view.

**Exit gate:** Running `tubeforge audit` prints a human-readable scorecard with trends.

---

## Phase 5 — Next-Topic Recommender

**Objective:** Database-driven content suggestions.

### 5.1 Inputs
- Own channel history (titles, tags, keywords, performance).
- Competitor high-performing videos (already ingested).
- Keyword research cache.
- Trending tags.

### 5.2 Algorithm (simple, transparent)
1. Collect keyword/tag candidates from competitors and research cache.
2. Filter out keywords already heavily covered by own channel.
3. Score each candidate by: competitor performance proxy, keyword opportunity, niche relevance.
4. Generate 3–5 title templates per candidate.
5. Persist recommendations in `next_ideas` with `generated_at`.

### 5.3 Module
- Extend `src/analytics/ideas.rs` with `recommend_next_topics(db)`.

### 5.4 CLI
- `tubeforge ideas next --channel <id>`

### 5.5 Dashboard
- `/ideas` page shows ranked next topics with title templates and expected SEO score.

**Exit gate:** Recommendations are explainable and stored for future comparison.

---

## Phase 6 — yt-dlp Public Enrichment (optional)

**Objective:** Add a public-only, auth-free fallback for video metadata.

### 6.1 Constraints
- Public pages only.
- No cookies, no `--cookies-from-browser`, no impersonation.
- Bounded concurrency (default 4 concurrent subprocesses).
- Only used when API is unavailable or user explicitly enables it.

### 6.2 Module
- `src/fetch/ytdlp.rs`
  - `YtdlpPublicClient { binary, semaphore, timeout }`.
  - `extract_one(url) -> Result<YtdlpVideoInfo>`.
  - Maps JSON fields to `VideoRow`.

### 6.3 CLI / config
- `.env` flag: `TUBEFORGE_YTDLP_ENABLED=true`.
- Optional binary path: `TUBEFORGE_YTDLP_PATH=yt-dlp`.

### 6.4 Dashboard
- Settings section to toggle yt-dlp enrichment.

**Exit gate:** yt-dlp can enrich a public video URL without auth and without breaking the default pipeline.

---

## Open questions

1. ~~Should keyword research use Google Trends as a second free signal?~~ → **Decided:** public RSS-based trending, added to Phase 3.
2. ~~Should thumbnail A/B testing be local-only (generate variants + pick before upload) or deferred until YouTube Analytics data is available?~~ → **Decided:** thumbnail extraction + per-video model, added to Phase 2.
3. ~~Do we want a simple MCP server so AI agents can query TubeForge directly?~~ → **Resolved (Aug 14, 2026):** no MCP. Agents connect via **stdio JSON-RPC** (`tubeforge rpc`, same method surface as the dashboard `/ws`) or the `--json` CLI envelope. The external `tursodb --mcp` was removed with the SQLite storage.

---

## SPA Dashboard Rewrite

**Decision date:** 2026-08-03
**Status:** Architecture finalized, scaffolding in progress.

### Why SPA (not htmx)
The current Askama + htmx + hand-rolled SVG charts are functional but not at TubeBuddy/VidIQ quality. The user explicitly wants:
- Real-time search/autocomplete with debounced API calls
- Modals/drawers for detail views (no full-page navigation)
- Drag-drop for tag reordering, idea prioritization
- Sortable/filterable data tables with column resize
- Dark/light theme toggle
- Mobile responsive layout
- Interactive charts (Chart.js) with tooltips, zoom, crosshair
- Keyboard shortcuts for power users

### Tech Stack

| Layer | Choice | Rationale |
|-------|--------|-----------|
| **Framework** | React 19 + Vite | Best ecosystem, widest chart/UI library support, SPA precedent |
| **Styling** | Tailwind CSS v4 | Utility-first, dark mode via class, component-friendly |
| **Charts** | Recharts | React-native, composable, good for dashboards |
| **Data Tables** | TanStack Table | Headless, sortable/filterable/paginated, column resize |
| **Routing** | React Router v7 | File-based, loaders, middleware support |
| **State** | TanStack Query + Zustand | Server cache + client state |
| **Notifications** | react-hot-toast | Lightweight, customizable |
| **Drag & Drop** | @dnd-kit | Modern, accessible, tree-friendly |
| **Icons** | Lucide React | Consistent, tree-shakeable |

### Project Structure

```
tubeforge/
├── frontend/                    # SPA (Vite)
│   ├── package.json
│   ├── vite.config.ts
│   ├── tailwind.config.js
│   ├── index.html
│   ├── src/
│   │   ├── main.tsx
│   │   ├── App.tsx
│   │   ├── routes/              # React Router pages
│   │   │   ├── Dashboard.tsx
│   │   │   ├── Videos.tsx
│   │   │   ├── Tags.tsx         # NEW: Tags Analyzer
│   │   │   ├── Keywords.tsx
│   │   │   ├── Scorecard.tsx
│   │   │   ├── Ideas.tsx
│   │   │   ├── Health.tsx
│   │   │   └── Alerts.tsx
│   │   ├── components/
│   │   │   ├── layout/          # Sidebar, Header, ThemeToggle
│   │   │   ├── charts/          # Chart wrappers
│   │   │   ├── tables/          # TanStack Table wrappers
│   │   │   ├── tags/            # Tag cloud, tag gap analysis
│   │   │   └── ui/              # Button, Modal, Drawer, Badge
│   │   ├── hooks/               # useDebounce, useKeyboard, useTheme
│   │   ├── lib/
│   │   │   ├── api.ts           # API client (fetch wrapper)
│   │   │   └── types.ts         # TypeScript types matching backend
│   │   └── styles/
│   │       └── globals.css      # Tailwind + custom
│   └── public/
├── src/serve/                   # raw-Hyper backend (existing)
│   ├── mod.rs                   # Add JSON API routes + static file serving
│   ├── api/                     # NEW: JSON API handlers
│   │   ├── mod.rs
│   │   ├── videos.rs
│   │   ├── tags.rs              # Tags Analyzer API
│   │   ├── keywords.rs
│   │   └── health.rs
│   └── ...                      # Existing HTML routes (kept for backward compat)
```

### Backend Changes

**New JSON API routes** added to existing raw-Hyper server:

| Method | Path | Returns | Description |
|--------|------|---------|-------------|
| `GET` | `/api/healthz` | `{ ok: true }` | Liveness |
| `GET` | `/api/counts` | `{ videos, channels, tags, ideas, alerts, keywords }` | Dashboard counters |
| `GET` | `/api/trends?period=1m` | `{ dates[], views[], subs[] }` | Trend charts |
| `GET` | `/api/alerts` | `Alert[]` | Alert list |
| `POST` | `/api/alerts/read` | `200` | Mark all read |
| `POST` | `/api/alerts/clear` | `200` | Clear all |
| `GET` | `/api/videos?q=&page=&sort=` | `{ items: Video[], total }` | Paginated video list |
| `GET` | `/api/videos/:id` | `VideoDetail` | Full video with scores |
| `GET` | `/api/scores?q=&sort=` | `Score[]` | Score table |
| `GET` | `/api/scores/:id` | `ScoreDetail` | 18 SEO + 7 GEO component breakdown (+ graph_scores) |
| `GET` | `/api/ideas` | `Idea[]` | Idea list |
| `POST` | `/api/ideas/:id/status` | `200` | Set idea status |
| `GET` | `/api/keywords` | `Keyword[]` | Keyword rank/trend |
| `GET` | `/api/keywords/trending` | `TrendingKeyword[]` | Keyless SERP-derived trending keywords (Google Trends blocked from this IP) |
| `GET` | `/api/tags` | `{ cloud, gaps, suggestions }` | Tags Analyzer aggregate |
| `GET` | `/api/tags/video/:id` | `VideoTags` | Tags for one video |
| `GET` | `/api/tags/competitor/:id` | `CompetitorTags` | Competitor tag comparison |
| `GET` | `/api/scorecard` | `Channel[]` | Competitor scorecard |
| `GET` | `/api/health` | `HealthReport` | System health |
| `GET` | `/events` | SSE stream | Real-time updates |

**Static file serving:**
- Development: Vite dev server on `:5173` with proxy to the backend `:17487`
- Production: `hyper` static file serving serves `frontend/dist/` with SPA fallback to `index.html`

**Route ownership (decided Aug 4, 2026):** when `frontend/dist/` exists the React SPA owns all root page routes (`/`, `/scores`, `/scores/{id}`, `/ideas`, `/alerts`, `/keywords`, `/scorecard`, `/health`); the legacy HTMX pages move under `/legacy/*`. Without a SPA build the HTMX pages keep the root routes. `/api/*`, `/events`, `/healthz`, `/static/*` are shared by both UIs. (Implementation: `src/serve/mod.rs::app()`.)

**Existing HTML routes** kept for backward compatibility under `/legacy/*` (can be removed later).

### Dev Workflow

```bash
# Terminal 1: backend
cargo run -- serve --port 17487

# Terminal 2: Vite dev server (proxies API to backend)
cd frontend && bun run dev
```

Production build:
```bash
cd frontend && bun run build    # → frontend/dist/
cargo run -- serve              # Serves SPA from frontend/dist/
```

### Tags Analyzer — Parallel Track

Built in parallel with the SPA scaffold. Backend + frontend together.

**New DB tables (migration 005):**
- `tags(id, name UNIQUE)` — tag vocabulary
- `video_tags(video_id, tag_id, position, source)` — video-tag mapping
- `competitor_tags(channel_id, tag_name, video_count, avg_views, rank)` — aggregated competitor tag data

**New backend modules:**
- `src/analytics/tags.rs` — tag cloud, gap analysis, per-video tags, competitor comparison
- `src/commands/tags.rs` — `tags backfill` CLI populates normalized tables from stored video data (needs exclusive DB lock; stop the server first)

**New SPA pages:**
- `/tags` — Tag cloud visualization, frequency chart, gap analysis, tag suggestions per video
- Tags section within `/scores/:id` — individual video tag breakdown

**Exit gate:** Tags page shows tag cloud + competitor gaps + suggestions, all interactive.

**Status (Aug 4, 2026):**
- ✅ Migration 005 + tag row types + Db methods (`count_tags`, `tag_cloud`, `tag_gaps`, `upsert_tags`, `upsert_competitor_tags`, `get_video_tags`, `get_competitor_tag_stats`)
- ✅ `src/analytics/tags.rs` (tag_cloud, tag_gaps, video_tags, competitor_tags, backfill_tags) wired into `/api/*` handlers
- ✅ `tubeforge tags backfill` CLI command (validated: `{"backfilled_videos":0,"total_tags":0}` on current corpus — live data ingested via RSS stores `"tags":"[]"`; tags exist only on `--api`-enriched videos)
- ✅ `/api/counts` `tags` field fixed (was mislabeled to the keywords count)
- ⏭ Backfill is a no-op until corpus is re-ingested with `--api` (or a tag-source command is added)

---

## VidIQ/TubeBuddy Benchmark — Gap Analysis (Aug 5, 2026)

**Sources studied (fetched this session):** vidIQ official Scorecard docs (support.vidiq.com), vidIQ "Understanding YouTube Algorithm 2026" blog, HashtagNetwork "VidIQ vs TubeBuddy" comparison, Alan Spicer (ex-vidIQ team) "vidIQ SEO Score Explained."

**What their SEO score is:** a *metadata audit* — how well YouTube can understand *what the video is about* from title/description/tags. Not content quality, not engagement. vidIQ: 50% actionable (creator-controlled) + 50% performance (ranked tags / high-volume tags). Alan Spicer's reverse-engineered weights: **keyword-in-title 30%, description quality 25%, tag strategy 20%, tag relevance 15%, hashtags 10%**.

### Gap table — our system vs the benchmark

| # | Capability (benchmark) | TubeForge today | Gap | Priority |
|---|---|---|---|---|
| 1 | **Keyword in first 40 chars of title** | `title_front` exists but checks *first token position ≤3*, not the 40-char window; no explicit "keyword within first 40 characters" signal | **Partial** — need a `title_40_chars` component mirroring vidIQ | High |
| 2 | **Keyword in description first 2 lines** | `desc_first150` checks first 150 chars — close but not "first two lines" (Alanic says first two lines is what YouTube crawls) | **Minor** — align semantics + add line-based check | Medium |
| 3 | **Description length 200+ words** | `desc_structure` checks structure (lines/bullets/hashtags/steps) but has NO length signal | **Missing** — add `desc_length` component (200+ words sweet spot) | High |
| 4 | **Tag count 15–30 (vidIQ 5+, Alan 15-30)** | `tags_quality` gives +50 for count in [3,8] — contradicts both sources (5 min per vidIQ; 15-30 target per Alan) | **Wrong band** — rework to [5..30] with 15-30 optimal | High |
| 5 | **Tag relevance (industry-standard tags)** | `tags_relevance` = Jaccard vs own title/desc — measures self-consistency, NOT niche-standard tags | **Different signal** — need corpus/competitor-frequency tag scoring (partially covered by new tags analyzer) | Medium |
| 6 | **Hashtags 3–5 in description** | `desc_structure` gives +25 for *any* hashtag presence — no count band | **Partial** — add 3-5 optimal count component | Medium |
| 7 | **"Triple keyword" (same keyword in title+tags+description)** | No cross-placement signal | **Missing** — add `keyword_triple` component | High |
| 8 | **Ranked tags / position in YouTube search** | `keyword_rankings` tracks position via corpus rank — but that's *our* BM25 rank, not YouTube search position | **Partial** — yt-dlp/API could give real search position later | Low |
| 9 | **High-volume tags (hot topics)** | No search-volume data at all (YouTube doesn't expose it; vidIQ estimates via clickstream) | **Missing** — best local proxy: competitor tag frequency + outlier topics | Medium |
| 10 | **Views-per-hour (VPH) / trending-now** | `view_count` stored but no VPH computation or recency-weighted trend | **Missing** — compute VPH from view_count/published age; flag "trending now" | High |
| 11 | **Outlier vs channel baseline** | ✅ **Delivered** in Phase 6.5 (`gaps::outliers`, 3x mean, verified live) | None | Done |
| 12 | **Session contribution / series formats** | No session concept; `graph` has PageRank but no "series" or "playlist" detection | **Missing** — series detection via title-pattern/episode regex; playlists not stored | Medium |
| 13 | **Engagement weighting: comments > likes (2024+)** | GEO/SEO scores ignore engagement entirely; no comment/like ratio signal | **Missing** — add engagement ratio (comments/likes/views) to a performance signal | High |
| 14 | **CTR benchmark (4%+)** | No CTR data (needs impressions — API doesn't provide; only own-channel analytics could) | **Not possible via public data** — document as out-of-scope; proxy = thumbnail contrast check | Low |
| 15 | **Retention 50%+ / AVD** | No retention data for competitors (API lacks it). ✅ **Heatmap now available via yt-dlp** (100-pt audience retention!) — but not yet stored/scored | **Partial** — persist heatmap, add retention-derived signals (hook drop-off, mid-roll dips) | High |
| 16 | **Channel age, historical subs/views graphs** | `channels` table stores subscriber_count/video_count snapshot but no history table | **Missing** — add `channel_snapshots` table for growth curves | Medium |
| 17 | **Title/thumbnail change tracking** | No tracking of metadata edits over time | **Missing** — add `video_metadata_history` on refresh diffs | Low |
| 18 | **Controversial keyword flagging** | Not present | **Missing** — simple sensitive-term list check on title/desc/tags | Low |
| 19 | **Keyword volume estimates + weighted score (TubeBuddy's "can you realistically rank")** | `ideas` has demand proxy (BM25 match count) but no channel-size-aware difficulty | **Partial** — add channel-authority × demand difficulty score | Medium |
| 20 | **Actionable pre-publish scorecard** | `tubeforge score --draft-*` exists and scores drafts ✅; but no "what's missing" checklist output | **Partial** — extend scorecard JSON with per-component recommendations | Medium |

### Phase 6.6 — Scoring refinement workstream (derived from the gaps)

**Objective:** align the SEO score with the industry benchmark (keyword-in-title 30% / description 25% / tags 20%+15% / hashtags 10% family), add the performance half (heatmap, VPH, engagement), and surface everything as an actionable checklist.

1. **SEO component rework** (`src/scoring/seo.rs`):
   - Add `title_40_chars` (keyword in first 40 chars — vidIQ's top signal)
   - Add `desc_length` (200+ words sweet spot)
   - Rework `tags_quality` band to [5,30] (15-30 optimal) — currently [3,8]
   - Add `desc_first2lines` (keyword in first two description lines)
   - Add `hashtag_count` (3-5 optimal)
   - Add `keyword_triple` (same keyword in title + tags + description)
   - Rebalance weights toward the vidIQ family (title+desc ≈ 55%)
2. **Performance half** (the 50% vidIQ says is performance):
   - Persist yt-dlp `heatmap` per video (migration 007: `video_heatmap` JSON) — currently captured but discarded
   - `vph` = view_count / hours-since-publish; `trending_now` flag (VPH > channel mean VPH × 3)
   - `engagement_ratio` = (comments×3 + likes×1) / views — reflects 2024 "comments > likes" weighting
   - `outlier_score` wired into the score detail (already computed in gaps.rs)
3. **Checklist output**: `tubeforge score --draft-*` gains a `recommendations[]` array ("Keyword not in first 40 chars of title", "Description under 200 words", "Add 15-30 tags", …) so the score is actionable, not just a number.
4. **Channel history** (migration 007): `channel_snapshots(channel_id, at, subscriber_count, video_count, total_views)` written on every refresh → growth graphs in the SPA.
5. **Series detection**: regex title-pattern clustering (episodes/parts/#N) → series membership in `video_tags`-style mapping; feeds the ideas engine and "session" suggestions.
6. **Engagement weighting** in ideas/gaps ranking: comment-rich outlier topics rank above view-only outliers (comments > likes per 2024+ weighting).

**Exit gate:** `tubeforge score --draft-*` on a sample draft returns the vidIQ-family weighted score + actionable recommendations; a competitor video with a stored yt-dlp heatmap shows retention-derived signals in its score detail; `channel_snapshots` grows on every `refresh`.

**Status (Aug 5, 2026):**
- ✅ SEO rework: 15 components (added `title_40_chars`, `desc_first2lines`, `desc_length`, `hashtag_count`, `keyword_triple`); `tags_quality` band reworked to [5,30] with 15-30 optimal; weights rebalanced (title+desc ≈ 67%, vidIQ family)
- ✅ Migration 007: `video_heatmap` + `channel_snapshots` (schema v7, verified on live DB)
- ✅ `src/analytics/performance.rs`: VPH, engagement_ratio (comments×3 > likes), heatmap retention (hook + mean), `mark_trending` (VPH > 3× channel mean), `persist_heatmap`
- ✅ `tubeforge metadata --video-id` (yt-dlp keyless): heatmap + live stats + channel followers persisted — verified live (100 points, 289k followers)
- ✅ `tubeforge score --draft-*` returns `recommendations[]` checklist (sorted by severity) — verified live
- ✅ `channel_snapshots` written on every `refresh` — verified live (15 videos, 1.76M views)
- ✅ Series detection in `gaps::coverage` (`is_series` flag); comments-weight engagement boost in `ideas` ranking
- ✅ **SPA surfaces (Aug 5):** `/scores/:id` detail page — audience-retention heatmap chart (Recharts), performance badges (VPH / engagement / retention / trending), 15 SEO + 7 GEO component bars with vidIQ-family labels; Scorecard page gains per-channel growth history line chart (total views + videos over `channel_snapshots`)
- ✅ API: `/api/scores/{id}` returns `performance` payload (heatmap points, vph, engagement_ratio/score, hook/mean retention, retention_score, trending); `/api/channels/{id}/snapshots` returns growth history — both verified live
- ✅ **Gap Mining UI (Aug 5):** `/api/gaps`, `/api/gaps/outliers`, `/api/gaps/coverage`, `/api/transcripts/{id}`, `/api/comments/{id}` endpoints; SPA `/gaps` page (outlier table with ×mean multiplier, coverage matrix with Short/series/gap-score columns, freshness + format gap lists); ScoreDetail gains transcript viewer + "Copy mining bundle" button — all verified live
- ✅ **Keyword Research (VidIQ Keyword-Inspector equivalent, Aug 5):** `tubeforge keywords inspect <kw>` + `/api/keywords/inspect?q=` — keyless real analysis: YouTube SERP demand proxy (ytsearch), competition (channel diversity × incumbent authority blend), opportunity score, related keywords (Google YouTube autocomplete), our-corpus matches (BM25). SPA Keywords page: input box → Opportunity/Competition gauges, best-fit verdict, "Ranking now" SERP list, related keywords chips, database matches. Calibration verified live: gradient across real keywords (22.4 rust async tokio → 0.3 tokio select macro). No LLM, no API key.
- ✅ **Full topic analysis (Aug 5, round 2):** when you type a topic you now get the complete VidIQ-class picture — **volume label** (Low/Med/High relative scale — exact numbers are unobtainable, TubeBuddy's honest approach), **activity signal** (ytsearchdate: recent uploads in 90d + actively_published flag), **suggested tags** (20 frequency-ranked real tags harvested from ranking videos), **SERP overlay** (each ranking video: views/likes/comments/upload date/real tags/our SEO score of their metadata), opportunity+competition gauges, plain-language verdict consistent with the volume bands, related keywords, corpus matches. Verified live in 16s (was 56s — fixed by forcing the android client for search; `player_client=all` multiplies cost 3× without adding fields). **Research finding:** Google Trends 12-month data (the last VidIQ signal) is blocked from this IP (HTTP 429 even with browser UA) — documented as unavailable; the recency/activity signal substitutes for it.
- ✅ **Research persistence + route fixes (Aug 5, round 3):** migration 008 `keyword_research` table — every `keywords inspect` call (API + CLI) persists its snapshot immediately; `/api/keywords/history?q=` returns the accumulated snapshots; SPA Keywords page charts opportunity/competition over time (the keyless substitute for VidIQ's 12-month trend graph) once ≥2 snapshots exist. **Frontend route bugs fixed:** `/videos` page created (searchable/sortable/paginated video table, was a dead sidebar link) and wired to a route; `/alerts` added to the sidebar nav (had a route+page but no nav item) — every nav item now has a live route.
- ✅ **Production VidIQ mimic (Aug 5, round 4):** **Channel Audit** (`src/analytics/audit.rs` + `/api/audit` + `/api/audit/{id}` + SPA `/audit` page) — VidIQ's flagship feature: composite 0-100 with grade (A-F) and six weighted components (metadata 30%, engagement 20%, consistency 15%, tags 15%, series 10%, authority 10%), each with detail + a weakest-lever verdict. Verified live on all 10 channels. **Keyword Inspector v2:** composite `keyword_score` headline (demand 40% × comp-inverted 35% × recency 15% × corpus-fit 10%), suggested tags now carry usage counts (×N ranking videos), related keywords carry Google popularity ranks (#1 = most-searched variant). **Videos list** now shows VidIQ-style SEO score badges per video (sortable). All keyless.
- ✅ **1000+ video corpus (Aug 5):** bulk-researched **215 tech topics** via `tubeforge keywords research` (135 base + 82 long-tail wave-2, CLI-driven with chunked batches + rate-limit cooldowns). Final corpus: **1,365 videos, 552 channels, 9,355 unique tags, 0 duplicates** (idempotent dedupe enforced per chunk). Analysis now runs on real scale: 45 outliers (incl. 20.8M-view course at 11.9×), 834 gap topics, 244 freshness gaps, research-history snapshots per topic. Bulk-research driver pattern documented: chunk ≤12 topics, `--serp 6`, file-redirect CLI output (piping to python loses stdout on macOS), 30-45s cooldown between chunks to avoid yt-dlp rate limits.
- ⏭ Next: none pending — Phase 6.6 exit gate met (recommendations verified live, heatmap scored, snapshots growing on refresh)

---

## Next action

**Status (Aug 14, 2026):** Phases 0–6 delivered (engine, ingest, SEO/GEO scoring, content layer, thumbnails, export, Knowledge Graph, agent hardening). **Current focus: Phase 4 hardening & release** — commit/push the engine-independence + stdio `rpc` work, then tag v0.1.0. Post-release: HNSW vector wiring (deferred), Wasm, cross-platform verification.
