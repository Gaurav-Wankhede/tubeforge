# TubeForge — Low-Level Design (LLD)

**Project:** TubeForge — local-first YouTube SEO/GEO growth engine
**Document version:** 1.6 | **Date:** August 14, 2026
**Status:** Approved — Phases 0–6 delivered; implementation reference for Phase 4+ (release hardening)
**Companion documents:** `PRD.md` (v4.0), `HLD.md` (v1.3)

> **v1.6 update (Aug 14, 2026):** sections updated for the engine-independence re-architecture — `tfdb` storage (custom `.wal`+`.dat`, no SQLite/rusqlite), from-scratch BM25 (`src/search`), raw-Hyper web framework + WebSocket JSON-RPC + SSE (`src/serve`), content/`analyze` layer, 18 SEO components, SCHEMA_VERSION 9. Phase 0 gate records the superseded v3 stack for reference.

---

## 1. Scope

This document specifies: module layout, data model (`tfdb` schema + own BM25 index spec), CLI contracts (commands, JSON envelope, exit codes, error taxonomy), fetch-layer design (RSS/oEmbed/API + quota), ingest pipeline semantics, scoring engine formulas, analytics modules, backup/recovery flows, concurrency model, configuration keys, migration/versioning, and the testing strategy.

**Grounding constraints (locked, evidence-backed, v1.6):**
- Engine: **`tfdb`** — from-scratch embedded store, custom `.wal` + `.dat` format. **Not** SQLite-compatible; no rusqlite escape hatch. Single-writer. (HLD §7, ADR-1)
- BM25: **TubeForge's own engine** (`src/search`, `k1=1.2, b=0.75`). Vector: HNSW module ships but is unwired (no embeddings). Graph: Rust PageRank + Louvain.
- Backup before every batch ingest (snapshot copy + integrity re-open).
- CLI-first v1; `--json` envelope; **stdio JSON-RPC** via `tubeforge rpc` (same method surface as `/ws`) for agent harnesses; WebSocket JSON-RPC + SSE via `serve`.

---

## 2. Module Layout (single crate, phase-gated)

```
tubeforge/
├── Cargo.toml                  # bin `tubeforge`; deps: hyper/hyper-util (raw-Hyper server),
│                               #   tokio, clap, reqwest (rustls, NOT native-tls), quick-xml,
│                               #   serde, serde_json, dotenvy, tracing, chrono, chrono-tz, url,
│                               #   thiserror, askama, chromiumoxide + fetcher, zip, tokio-tungstenite
├── .env.example
├── src/
│   ├── main.rs                 # tokio runtime, clap dispatch → exit(code)
│   ├── cli.rs                  # subcommand definitions (clap derive)
│   ├── config.rs               # .env load, TUBEFORGE_DB_PATH/backup/weights resolution
│   ├── error.rs                # TubeforgeError enum + exit-code mapping
│   ├── output.rs               # TableRenderer / JsonEnvelope
│   ├── fetch/
│   │   ├── mod.rs
│   │   ├── rss.rs              # feed fetch + parse (quick-xml)
│   │   ├── oembed.rs           # single-video metadata
│   │   ├── api.rs              # YouTube Data API v3 client (videos.list batching)
│   │   ├── quota.rs            # per-endpoint budget ledger (persisted in meta table)
│   │   └── ytdlp.rs            # transcripts (auto/manual subs→WebVTT→text), comments, heatmap, SERP research (opt-in)
│   ├── ingest.rs               # resolution, ID extraction, dedupe, upsert orchestration
│   ├── categories.rs           # YouTube category map (32 ids → names)
│   ├── tfdb/                   # from-scratch storage engine (pure Rust)
│   │   ├── mod.rs              # Engine, EngineOptions, Tx, Value; file layout (.wal/.dat)
│   │   ├── store.rs            # WAL append + fsync-on-commit, atomic checkpoint, replay, torn-tail truncation
│   │   ├── schema.rs           # Col/TableSchema (typed columns, no SQL DDL)
│   │   ├── query.rs            # sum/avg/min/max/group_counts/join scans
│   │   ├── tfdb_schema.rs      # 22 table definitions
│   │   ├── graph.rs            # property graph (typed nodes/edges)
│   │   └── hnsw.rs             # HNSW vector index (SHIPS, UNWIRED — no embeddings)
│   ├── storage/
│   │   ├── db.rs               # Db facade (Arc<Mutex<Engine>> + path); re-exports db_tf.rs
│   │   ├── db_tf.rs            # repository methods over tfdb engine
│   │   ├── schema.rs           # SCHEMA_VERSION + migration runner (version-gated)
│   │   └── backup.rs           # snapshot copy + integrity re-open + retention, restore
│   ├── search/
│   │   ├── mod.rs              # own BM25 engine (only module importing it)
│   │   ├── index.rs            # Index (Arc<RwLock<Store>>) + IndexWriter commit, rebuild
│   │   ├── store.rs            # in-memory inverted index → atomic checksummed index.json snapshot
│   │   └── bm25.rs             # BM25 scoring (k1=1.2, b=0.75), matches, corpus_resonance
│   ├── scoring/
│   │   ├── mod.rs              # compute / compute_with_graph entry points
│   │   ├── seo.rs              # 18 SEO components (10 structural + 5 vidIQ + 3 graph)
│   │   ├── geo.rs              # 7 free-signal GEO components
│   │   ├── psych.rs            # packaging-psychology TitleFormula + variants (supporting)
│   │   ├── graph_aware.rs      # compute_graph_scores (tag_authority/topic_dominance/keyword_competition)
│   │   ├── recommend.rs        # recommendation checklist
│   │   └── weights.rs          # weight config (defaults + .env overrides)
│   ├── analytics/
│   │   ├── mod.rs
│   │   ├── graph.rs            # channel adjacency + PageRank/Louvain helpers
│   │   ├── ideas.rs            # Next Ideas generation + ranking
│   │   ├── keywords.rs         # rank tracking snapshots
│   │   ├── growth.rs           # own-channel overview + keyword opportunities + next-video recs
│   │   ├── content.rs          # deterministic content draft generation (title/desc/tags)
│   │   ├── forecast.rs         # weighted OLS growth forecasting from channel_snapshots
│   │   ├── kg_builder.rs       # KG build (Full/Incremental) + load_or_build cache
│   │   ├── kg_algorithms.rs    # PageRank + Louvain over the graph
│   │   ├── topic_generator.rs  # greedy bot: candidate topics from autocomplete/competitor tags/drift
│   │   ├── history_tracker.rs  # greedy bot: cooldown, dedup, research history logging
│   │   └── reports.rs          # scorecard, health, alerts
│   ├── thumbnail/
│   │   ├── mod.rs              # template fill, render orchestration (chromiumoxide)
│   │   ├── render.rs           # CDP render → PNG 1280×720
│   │   └── assets.rs           # per-render temp dir + RAII cleanup guard
│   ├── export/
│   │   ├── mod.rs              # manifest.json + JSON arrays + zip/dir output
│   │   └── csv.rs              # videos/channels/tags/keywords CSV writers (escaping)
│   ├── serve/                  # dashboard server (PRD §5.4, v1.6 re-architecture)
│   │   ├── mod.rs              # loopback-only server bootstrap + routing
│   │   ├── web.rs              # raw-Hyper web framework (State/Query/Path/Headers extractors)
│   │   ├── api.rs              # HTTP API handlers under /api/*
│   │   ├── api/analysis.rs     # /api/analysis/* handlers
│   │   ├── rpc.rs              # JSON-RPC dispatch + handlers (transport-agnostic; shared by /ws and stdio)
│   │   ├── stdio.rs            # stdio JSON-RPC bridge (`tubeforge rpc`) — line-delimited stdin/stdout
│   │   ├── csrf.rs             # Origin/Referer CSRF guard for POSTs
│   │   ├── svg.rs              # server-rendered inline SVG charts (bars/histogram/sparklines)
│   │   └── templates.rs        # askama template types (compile-time autoescaping)
│   ├── templates/
│   │   ├── default.html input.css tailwind.css   # compiled Tailwind v4 CSS committed
│   │   └── dashboard/          # askama 0.14 HTML templates: base.html, home.html, home_counts.html,
│   │                           #   home_ideas.html, scores.html, score_detail.html, ideas.html,
│   │                           #   idea_row.html, keywords.html, scorecard.html, alerts.html,
│   │                           #   alerts_list.html, health.html, macros.html, not_found.html
│   ├── static/                 # vendored assets (offline, no CDN)
│   │   ├── htmx.min.js         # htmx 2.0.9 (legacy HTMX pages; retained)
│   │   └── sse.js              # SSE EventSource client helper (current dashboard path)
│   └── commands/               # one file per CLI subcommand
│       ├── init.rs ingest.rs refresh.rs score.rs ideas.rs keywords.rs
│       ├── tags.rs transcript.rs metadata.rs comments.rs gaps.rs outliers.rs
│       ├── scorecard.rs health.rs analyze.rs forecast.rs suggest.rs alerts.rs
│       ├── backup.rs quota.rs reindex.rs rpc.rs thumbnail.rs availability.rs
│       ├── videos.rs export.rs filmot.rs prompt.rs serve.rs
│       └── greedy.rs           # greedy bot: run, status, seeds, daemon, stop
└── tests/
    ├── fixtures/               # local HTTP server (wiremock) RSS/oEmbed/API payloads
    └── *.rs                    # integration + property tests (incl. serve.rs — dashboard HTTP suite)
```

**Dependency rule:** `commands` → domain modules → `storage`/`tfdb` + `search` (leaf). `storage`/`tfdb` is the only storage engine; `search` is the only BM25 module. No SQL engine, no external index/web framework.

---

## 3. Data Model

### 3.1 `tfdb` schema (from-scratch engine, SCHEMA_VERSION = 10)

Storage is a **typed-row key/value model** (`src/tfdb/schema.rs`): each table has a fixed `TableSchema` with `Col { name, ColType }`; rows are `BTreeMap<String, Value>`; composite keys are folded into a single primary-key string (e.g. `keyword\x1fchecked_at`, `from\x1fto`); auto-increment ids are assigned in Rust. **No SQL DDL.** The conceptual tables (25 — see PRD §15) include:

```text
channels          (channel_id PK, handle UNIQUE, title, description, avatar_url, country,
                   subscriber_count, video_count, source rss|api, etag, fetched_at, updated_at)
videos            (video_id PK, channel_id, title, description, tags JSON[], category_id,
                   duration_sec, published_at, view_count, like_count, comment_count,
                   recording_date, recording_location_name, recording_lat, recording_lng,
                   topic_categories JSON[], thumb_url, embedding BLOB [unused — lexical only],
                   source rss|oembed|api, privacy_status, fetched_at, updated_at)
competitors       (channel_id PK, label, added_at)
keywords          (keyword PK, niche, created_at)
keyword_rankings  (keyword\x1fchecked_at PK, video_id, position [NULL = not found], topics JSON)
scores            (video_id PK, seo_score, geo_score, total_score, components JSON, computed_at)
ideas             (idea_id autoincrement, title_suggestion, rationale JSON, score, status
                   draft|saved|discarded, source_video, created_at)
edges             (from_channel\x1fto_channel PK, weight, source overlap|manual)   -- competitor graph
alerts            (alert_id autoincrement, kind brand|gap|quota|integrity, channel_id, message,
                   severity info|warn|critical, created_at, read_at)
ingest_log        (batch_id\x1fseq PK, item, status ok|skipped|failed, detail, at)
tags / video_tags / competitor_tags   -- tag entities + membership
transcripts       (video_id, lang, source, text, word_count)    -- yt-dlp captions
comments          (video_id, ...)                               -- yt-dlp comments (opt-in)
video_heatmap     (video_id, ...)                               -- yt-dlp live stats/heatmap
channel_snapshots (channel_id, subscribers, video_count, total_views, at)   -- growth history
keyword_research  (topic, ...)                                  -- analyze/SERP research
kg_entities       (entity_id, entity_type, canonical_name, display_name, properties JSON,
                   embedding BLOB [unused], centrality, community_id, source, source_ref)
kg_relations      (from, to, relation_type, weight, source)     -- weighted graph edges
kg_communities    (community_id, community_type, summary, member_count, mean_views,
                   mean_seo_score, top_entities)
greedy_seeds      (seed_id autoincrement, seed, source, added_at, active)
greedy_research_history (research_id autoincrement, topic, researched_at, video_ids_json,
                         video_count, mean_views, source, duration_ms)
greedy_topic_log  (log_id autoincrement, topic, status, reason, attempted_at)
meta              (key PK, value)  -- schema_version, quota_*, last_backup_at, last_reindex_at,
                                    -- settings_json, kg_cache_json
```

### 3.2 Own BM25 index spec (`src/search`)

- **Location:** `<data>/index/index.json` (atomic, checksummed snapshot; rebuildable — not part of backups).
- **Schema fields:** `video_id`, `channel_id`, `title`, `description`, `tags` (joined), `published_at` (3 tokenized/queryable: title/desc/tags).
- **Tokenizer:** lowercase + split on non-alphanumeric.
- **BM25:** `k1=1.2`, `b=0.75`, field-specific doc-length normalization; `COLLECT_LIMIT = 10_000`.
- **Query surface (bm25.rs):**
  - `matches(q)` — best-first `(video_id, f32)`
  - `corpus_resonance(q, self_exclude?)` — max BM25 across corpus (optionally self-excluding a video)
  - `has_term`, `terms`, `num_docs`
- **Lifecycle:** `IndexWriter` accumulates upserts per ingest batch, `commit` persists atomically; full rebuild = drop snapshot + re-index from `videos` (idempotent — recovery path for any index inconsistency). `reindex` command.

### 3.3 Rationale notes

- **No external index engine** — tantivy and engine FTS were removed (ADR-2); BM25 is TubeForge-owned (`src/search`).
- **Embeddings column: reserved, unused in v1.** Lexical-only (ADR-9); HNSW module ships but no embeddings are generated. Semantic embeddings can be added later **without a schema change** (the `embedding` BLOB columns already exist on `videos` and `kg_entities`).
- **`meta.schema_version`** drives migrations (section 9). **SCHEMA_VERSION = 10**.
- **Ingest idempotency:** `video_id` PK → upsert semantics (see 6.2).

---

## 4. CLI Contracts

### 4.1 Commands (v1)

| Command | Purpose | Key flags |
|---|---|---|
| `init` | Create data root, `.env` scaffold, DB + migrations, test open | `--db-path` |
| `ingest channels <ref>...` | Resolve + fetch + upsert channels (RSS, optional API) | `--api`, `--no-backup` |
| `ingest links` | Read multi-line video URLs from stdin/`--file` → IDs → oEmbed/API | `--file -`, `--api` |
| `refresh` | Re-fetch known channels (ETag-aware) | `--channel <id>...` |
| `score` | Score a draft (title/desc/tags args) or stored video | `--video-id`, `--draft-title`, `--draft-desc`, `--draft-tags`, `--json` |
| `ideas` | Rank Next Ideas | `--limit 10`, `--niche`, `--status` |
| `keywords add <kw>...` / `keywords check` / `keywords ranks` | Track keyword positions | `--json` |
| `scorecard [<channel>...]` | Competitor comparison | `--json` |
| `health` | Data completeness, quota, integrity summary | `--json` |
| `alerts [--mark-read]` | Brand/coverage alerts | `--json` |
| `backup` | VACUUM INTO + integrity_check + retention prune | `--to <dir>` |
| `quota` | Show YouTube API usage | `--json` |
| `reindex` | Rebuild own BM25 index from `videos` | — |
| `thumbnail render` | Render template → 1280×720 PNG; raw assets in per-render temp dir, deleted after success (RAII guard; `--keep-assets` debug-only) | `--template`, `--title`, `--output`, `--keep-assets`, `--json` |
| `thumbnail list-templates` | List available HTML+Tailwind templates | `--json` |
| `check availability` | Batched `videos.list` (part=snippet,status); missing IDs → `video_unavailable` alerts; records `privacy_status` | `--json` |
| `export` | Export DB to `--format zip\|dir`: manifest.json, videos.csv (19 cols), channels.csv, tags.csv, keywords.csv, keyword_rankings.csv + JSON arrays (videos, ideas, alerts, scores); deterministic zip archives | `--format`, `--output`, `--json` |
| `filmot get` | Opt-in recovery lookup via Filmot API (`TUBEFORGE_FILMOT_KEY`); raw JSON passthrough + tolerant summary; no DB writes; non-fatal. Empty key → exit 1 CONFIG error | `--video-id`, `--json` |
| `analyze <topic>` | Realtime yt-dlp SERP research → demand-supply gap + auto-drafted title/desc/tags (content::generate) + ranking chart + packaging | `--json` |
| `forecast` | Growth forecast from `channel_snapshots` (weighted OLS + recency half-life) | `--horizon`, `--channels`, `--json` |
| `suggest <topic>` | Next-video recommendations (forecast-ranked, excludes covered topics, view-prediction tier + "why") | `--json` |
| `tags backfill\|analyze` | Backfill tag entities; analyze tag coverage/gaps | `--json` |
| `transcript get\|list\|clear` | yt-dlp caption extraction (auto/manual subs → WebVTT→text) → `transcripts` table | `--json` |
| `metadata` | Video heatmap / live stats via yt-dlp | `--json` |
| `comments get\|list\|clear` | yt-dlp comment extraction (opt-in) → `comments` table | `--json` |
| `gaps [--channel]` / `gaps outliers` | Content/tag gap analysis (incl. graph gaps when KG built) | `--channel`, `--markdown`, `--json` |
| `videos dedupe` | Detect/merge duplicate videos | `--json` |
| `rpc` | Stdio JSON-RPC bridge for agent harnesses (OpenCode, Claude Code, Codex, Hermes, Pi Agent): reads one request per stdin line, streams responses to stdout. **Long-running — stdout reserved for responses; never emits the JSON envelope.** Same method surface as `/ws` | — |
| `prompt` | Print agent/usage prompt | — |
| `serve` | Dashboard (PRD §5.4, v1.6): raw-Hyper web framework + WebSocket JSON-RPC (`/ws`) + SSE (`/events`), bind loopback, serve until Ctrl-C. **Long-running — does NOT emit the JSON envelope; stdout stays empty** (listening line → stderr; `--json` ignored). One shared Db; single-writer caveat — do not run concurrently with writing CLI commands (snapshot readers fine) | `--port` (default 8080; `TUBEFORGE_SERVE_PORT` overrides), `--host` (loopback only: `127.0.0.1`/`localhost`/`::1`; non-loopback → rejected, exit 2) |

Global: `--json`, `--verbose`, `--db-path`, `--config <env file>`.

### 4.2 JSON envelope (stable contract)

```json
{ "ok": true,
  "data": { "...": "..." },
  "meta": { "duration_ms": 42,
            "quota": { "videos_list_used": 3, "daily_limit": 10000 } } }

{ "ok": false,
  "error": { "code": "QUOTA_EXHAUSTED", "message": "…",
             "source": "youtube-api", "item": "UC_x5XG1OV2P6uZZ5FSM9Ttw" } }
```

`score --json` data: `{ video_id?, seo: {total, components{...}}, geo: {total, components{...}}, total }`.

### 4.3 Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Runtime/storage error |
| 2 | Usage error (clap) |
| 3 | Fetch/network error (retries exhausted) |
| 4 | Quota exhausted (operation degraded or aborted) |
| 5 | Integrity failure (`integrity_check` failed → backup/restore advised) |

### 4.4 Error taxonomy (`error.rs`)

```rust
enum TubeforgeError {
  Config(String),                    // missing .env key, bad path
  Fetch { source: Source, url: String, inner: String },   // Source: Rss|OEmbed|Api
  Parse { source: Source, item: String, inner: String },
  Quota { endpoint: Endpoint, remaining: u64 },
  Storage { code: String, message: String },   // engine error passthrough
  Integrity { detail: String },               // → exit 5
  Index { detail: String },                   // BM25 index errors
  Usage(String),                              // → exit 2
}
```
Mapping: `From<TubeforgeError> for i32` centralizes exit codes. Errors always render in the JSON envelope under `error`, and human mode prints `code: message` on stderr.

### 4.5 Dashboard (`serve` — PRD §5.4, delivered; re-architected Aug 14, 2026)

**Stack:** **raw-Hyper web framework** (`serve/web.rs` — plain `(Method, PathPattern) -> handler`, extractors `State/Query/Path/Headers/ReqUri`, `{param}` path segments, built on hyper/hyper-util; **no Axum**) + askama 0.14 templates (compile-time autoescaping) + **WebSocket JSON-RPC** (`serve/rpc.rs`) + **SSE** (`serve.rs::events`). Charts are server-rendered inline SVG from Rust (`serve/svg.rs`) — **no JS chart libraries** (PRD §11 resolved).

**HTTP API + RPC surface** (mounted under `/api/`; full list in PRD §5.4/§13):

| Path | Content |
|---|---|
| `/` | Dashboard home — health cards + SSE counts + SVG charts |
| `GET /events` | SSE stream (`text/event-stream`): `counts` event on change every 5s, 15s `: ping` heartbeat |
| `WS /ws` | WebSocket JSON-RPC — envelope `{id, method, params}`; out `progress {id, progress, message}` → `result {id, data}` \| `error {id, error:{code,message}}` \| `notification {event, data}`. ~21 methods (dashboard.overview, ideas.analyze, keywords.*, scores.*, videos.*, scorecard.get, health.get, gaps.*, tags.*, analysis.*, alerts.*, audit.get, channels.snapshots). Errors `-32700` parse / `-32603` internal. `RuntimeScorer` recomputes SEO+GEO fresh from the BM25 index; `analysis.refresh` does live yt-dlp fetch on a dedicated std thread |
| `GET /api/healthz` | Plain `ok` — liveness probe |
| `GET /api/health` | Health report page/census |
| `/api/scores` `/api/scores/{id}` | Scores list + component drilldown (18 SEO + 7 GEO + graph_scores) |
| `/api/ideas/analyze`, `/api/keywords*`, `/api/scorecard`, `/api/audit`, `/api/gaps*`, `/api/transcripts`, `/api/comments`, `/api/tags*`, `/api/analysis/*`, `/api/channels/{id}/snapshots` | Dashboard datasets |
| `/static/htmx.min.js` | Vendored htmx 2.0.9 (legacy HTMX pages; offline, no CDN) |
| `/static/sse.js` | Vendored SSE EventSource client helper (current dashboard path) |

**CSRF policy** (`serve/csrf.rs`): loopback server has no auth — remaining risk is local CSRF (malicious webpage POSTing to `127.0.0.1:<port>`). Origin guard on POSTs: `Origin`/`Referer` host:port must match the bound address (`localhost` ≡ `127.0.0.1`; scheme http/https) → mismatch or unparseable (`Origin: null`) → **403**. Neither header present → allowed (curl/scripts/AI agents send no Origin and can't be browser-CSRF'd).

**Concurrency (single-writer caveat):** `serve` opens ONE shared Db (app state) and mutates only via existing CLI repository methods (`set_idea_statuses`, `mark_alerts_read`, `clear_alerts`) — no duplicated write logic. Running `serve` concurrently with writing CLI commands is **unsupported**; concurrent readers fine (snapshot/in-memory reads).

**stdout purity:** `serve` is long-running and never emits the JSON envelope (LLD §4.2 applies to one-shot commands); stdout stays empty (smoke-verified 0 bytes), listening line → stderr. `--json` global flag is ignored for `serve` (documented in `cli.rs` help).

**Loopback enforcement:** bind host checked at startup — `127.0.0.1`/`localhost`/`::1` only; any other host → usage error, exit 2. Port precedence: `--port` flag > `TUBEFORGE_SERVE_PORT` env > 8080.

### 4.6 Stdio JSON-RPC bridge (`rpc` — agent-harness connection, PRD §5.9/§13)

**Purpose:** agent harnesses (OpenCode, Claude Code, Codex, Hermes, Pi Agent, ...) connect to TubeForge for **analysis** via JSON-RPC over **stdio** — not a separate network server, not MCP, not the `prompt` command. The tfdb database is the storage source; the frontend dashboard provides visual analysis.

**Design:** `serve/stdio.rs` — one transport task wraps `stdout`, the shared `serve::rpc::dispatch` handles methods. It reuses the exact WebSocket method surface, so the protocol is **one JSON-RPC interface, two transports** (`/ws` for the dashboard, stdio for agents).

**Contract (line-delimited):**
```
stdout-of-binary input  →  {"id":"r1","method":"scores.list","params":{}}
binary stdout          →  {"id":"r1","type":"progress","progress":0.2,"message":"..."}
binary stdout          →  {"id":"r1","type":"result","data":{...}}
parse error            →  {"id":null,"type":"error","error":{"code":-32700,"message":"..."}}
unknown method         →  {"id":"r1","type":"error","error":{"code":-32603,"message":"..."}}
```

- **stdout purity:** stdout carries **only** one JSON-RPC response per line (flushed per message); all diagnostics/logs go to stderr. Like `serve`, it never emits the LLD §4.2 JSON envelope.
- **Concurrency:** single-writer Db shared with `serve`; `dispatch` is sequential per request. Do not run `rpc` concurrently with writing CLI commands.
- **Lifecycle:** runs until stdin EOF → exit 0. Special-cased in `main.rs` (`run_rpc`), bypassing the envelope pipeline.
- **Tests:** `agent_contract_stdio_rpc` (spawns `tubeforge rpc`, writes requests, asserts clean JSON responses + error codes on stdout, exit 0 on EOF).

---

## 5. Fetch Layer

### 5.1 RSS (`fetch/rss.rs`)
- URL: `https://www.youtube.com/feeds/videos.xml?channel_id=<id>` (verified live Aug 2026; undocumented endpoint — treat as best-effort, must degrade gracefully).
- Parse (quick-xml): entry → `video_id`, `title`, `link`, `published`, `updated`, `media:description`, `media:thumbnail`, `media:starRating`, `media:statistics views`.
- **ETag caching:** store feed etag in `channels.etag`; send `If-None-Match`; 304 → no-op refresh.
- **Known limit:** ~15 most-recent entries per channel (documented; history requires API key or repeated polling).
- Timeout: 15s; retry: 3× exponential backoff (500/429/timeout).

### 5.2 oEmbed (`fetch/oembed.rs`)
- URL: `https://www.youtube.com/oembed?url=https://www.youtube.com/watch?v=<id>&format=json` (verified live).
- Fields: `title`, `author_name`, `author_url`, `thumbnail_url` (480×360 max), `html`. **No description/views/date** — never claim rich metadata without an API key.
- Used for single-link ingestion without key.

### 5.3 YouTube Data API v3 (`fetch/api.rs`)
- **Only** `videos.list` (1 unit/call, ≤50 `id` params per call — verified quota docs). **Never** `search.list` (separate 100-calls/day bucket).
- Batch strategy: chunk ≤50 IDs → one call → merge; track units in `meta` (`quota_videos_list_used` + date; reset at midnight PT per quota docs).
- `part=snippet,contentDetails,statistics` with `fields` projection to bound payloads.
- Error handling: `403 quotaExceeded` → set degraded flag, fall back to RSS/oEmbed for the remainder, emit alert `kind=quota`.
- Auth: `key=<YOUTUBE_API_KEY>` query param from `.env` only.

**API behavior notes (recorded from MW Metadata wiki research, Aug 2026 — affects fetch planning):**
- `search.list` has an undocumented ~750-result cap (~15 pages) despite `totalResults` showing more.
- `playlistItems.list` caps at 20,000 videos per playlist (non-uploads playlists max ~5,000); RSS `feeds/videos.xml` has no such cap.
- `commentThreads.list` is effectively unlimited (11M+ comments observed; 100/page, 1 unit each).
- Missing `statistics` fields = disabled metric (uploader choice); `madeForKids` / auto-generated " - Topic" channels / private-deleted → `commentsDisabled` / `videoNotFound` error reasons.
- `recordingDetails` / `topicDetails` parts each cost quota per-part per-call by Google's model; TubeForge bills conservatively at 1 unit/call (documented in `src/fetch/api.rs` comment).

**Availability check (`check availability`, Phase 3):** batch `videos.list` with `part=snippet,status` (≤50 IDs/call); any ID returned missing → `video_unavailable` alert; `privacy_status` from `status.privacyStatus` persisted per video (migration 003).

**Filmot recovery (`filmot get`, Phase 3):** opt-in third-party service — `GET filmot.com/api/getvideos?id=<id>&flags=1&key=<TUBEFORGE_FILMOT_KEY>`; raw JSON passthrough + tolerant summary only; never writes DB; network/parse failures non-fatal; empty `TUBEFORGE_FILMOT_KEY` → CONFIG error (exit 1) with setup hint.

### 5.4 Quota ledger (`fetch/quota.rs`)
- Persist per-day usage in `meta`; `quota` command renders used/limit per endpoint.
- Pre-flight check before batches: if projected cost > remaining → warn + degrade (or abort with exit 4 for `--api` required paths).

---

## 6. Ingest Pipeline

### 6.1 Input resolution
- Video URL → ID: regex `(?:v=|shorts/|youtu\.be/)([A-Za-z0-9_-]{11})` — extended (Phase 2) to `/v/ /embed/ /shorts/ /video/ /watch/ /live/` paths, `youtu.be`, playlist prefixes `UU|UUSH|PL|FL|SP|OLAK`, and channel `UC|SC`.
- Bare-ID checksum validation (archiveteam-derived): video `^[A-Za-z0-9_-]{10}[AEIMQUYcgkosw048]$`, channel `^[A-Za-z0-9_-]{21}[AQgw]$` — checksum applies to bare IDs only (URL captures authoritative); invalid IDs labeled `rejected` in `--json` output + `ingest_log`.
- Channel ref → id: `@handle` → RSS probe (`feeds/videos.xml?handle=@x` fails) → resolve via API `channels.list(forHandle)` when key present; `UC...` accepted directly; URL `youtube.com/@x` → extract handle; `/user/` `/c/` `@handle` forms and `SC→UC` transform for `/show/` channels.
- Multi-line text input (`ingest links`): blank-line separated, comments `#` allowed.

### 6.2 Upsert semantics (idempotent)
- `INSERT ... ON CONFLICT(video_id) DO UPDATE` for `videos` (never delete on re-ingest; keep history).
- Channel upsert likewise; new channel → auto-insert, mark competitor only if explicitly added (`ingest channels --competitor` or `competitors` insert).
- Source precedence on conflict: `api` > `oembed` > `rss` (rich data wins; never downgrade).
- oEmbed carries no publish date → `videos.published_at` = ingest time for oEmbed-sourced rows (documented limitation, LLD §5.2).

### 6.3 Transaction + ordering rules (`tfdb` constraint)
- **Single writer, single transaction.** `tfdb` serializes writers with one write lock; pipeline is strictly sequential: fetch-all → single transaction (`begin`→`Tx` → `commit` writes one WAL record + fsync) → single writer. Reads from the in-memory snapshot are fine between writes.
- Entire batch in one transaction; on any failure → rollback, log failed items to `ingest_log`, exit 1.
- **Backup guard:** automatic `backup` (snapshot copy + integrity re-open) before every batch write unless `--no-backup`. Integrity failure → abort with exit 5 (never write into a corrupt DB).

### 6.4 Post-ingest
- BM25: delete stale docs for updated videos + add new (one `IndexWriter.commit`).
- Scoring: recompute `scores` only for changed/inserted videos.
- `ingest_log` rows per item; summary counts → output.

---

## 7. Scoring Engine

### 7.1 Pipeline
```
Inputs: title, description, tags[], channel context, target keywords (optional)
Signals → components → weighted total (0–100) → components JSON persisted
```
> **Phase 2 status (Aug 4, 2026):** 10 SEO + 7 GEO components (incl. C1/C2 `location_signal` / `topic_relevance`), k-scaled formulas, baked defaults + env overrides; Phase 1 BASIC mode superseded. **Phase 6.6 update (Aug 14, 2026):** extended to **18 SEO components** (5 vidIQ/Phase-6.6 + 3 graph) + **packaging-psychology** supporting layer. Entry points: `compute`, `compute_with_meta`, `compute_with_graph` (`src/scoring`).

### 7.2 SEO components (default weights; override via env) — 18 total

**Structural (10, from Phase 2):**

| Component | Signal(s) | Formula sketch (v1, deterministic) |
|---|---|---|
| `keyword_title` | BM25 title score vs target keyword(s) | `min(1, bm25_title / k)` × 100 |
| `title_front` | keyword position in title | `pos<=3 → 100; <=7 → 70; else 40` |
| `title_length` | character count | ideal 40–60 → 100; piecewise falloff |
| `title_hooks` | numbers, power words, "how to", brackets | +X per hit (capped) |
| `keyword_desc` | BM25 description score | scaled as above |
| `desc_first150` | keyword in first 150 chars | boolean-ish 100/60/0 |
| `desc_structure` | newlines, bullets, hashtags, ≥2 lines | checklist score |
| `tags_relevance` | tags ∩ (title+desc) tokens | Jaccard/TF overlap × 100 |
| `tags_quality` | count in [3,8], order matches content | checklist |
| `keyword_tags` | target keyword present among tags | 100/60/0 |

**vidIQ / Phase-6.6 (5):**

| Component | Signal(s) | Formula sketch |
|---|---|---|
| `title_40_chars` | first 40 chars readability (mobile truncation) | checklist |
| `desc_first2lines` | keyword + hook in first 2 desc lines | checklist |
| `desc_length` | description length target band | piecewise |
| `hashtag_count` | hashtag count target band | piecewise |
| `keyword_triple` | keyword in title+desc+tags (triple presence) | boolean-ish |

**Graph (3, via `graph_scores` — from Knowledge Graph):**

| Component | Signal(s) | Formula sketch |
|---|---|---|
| `tag_authority` | mean authority of video's tags weighted by channel centrality | `Σ(tag_authority_i) / n` |
| `topic_dominance` | max dominance across title tokens for the video's channel | `max(dom_i)` |
| `keyword_competition` | max channel dominance over the keyword's edges (high = competitive) | `max(dom_edge)` |

> **Note:** `SEo_COMPONENT_KEYS` surfaced via API/RPC carries the **15** non-graph components; the 3 graph components flow separately through `graph_scores`. Runtime fresh scores with graph=None produce graph components = 0; stored scores via `compute_with_graph` persist the full 18. Component-count inconsistency is a known documented surface (PRD §10, R6).

### 7.3 Packaging-psychology (supporting, NOT blended into totals)

`psych.rs` — five `TitleFormula` patterns, `score() = 20 pts/detected formula capped 100`: `TimeAnchor`, `PreciseNumber` (+extreme-outcome bonus), `IncomeClaim`, `ForbiddenKnowledge`, `HowToIdentity`. `variants()` generates Martell/Hormozi-style titles.

### 7.4 GEO components (free signals only — no paid API)

| Component | Signal | Formula sketch (v1, deterministic) | Rationale |
|---|---|---|---|
| `entity_coverage` | who/what/when/where/how present in desc | `n_present/5 × 100` | AI answer engines cite complete entities |
| `qa_phrasing` | question-style headings ("What is…?") | question-form heading → 100/60/0 | Answer-shaped content gets cited |
| `list_phrasing` | lists/bullets/step markers | checklist: bullets/lists/steps present | Extractable answers |
| `conversational` | natural tone markers vs keyword-stuffing (density ceiling) | density past ceiling → linear penalty | Over-optimization penalty |
| `metadata_complete` | desc, tags, timestamps present | checklist score (0–100) | Structured completeness |
| `location_signal` | `recordingDetails` lat/lng or location name (C1, api only) | `70` when lat/lng or location name present; `+30` when `recordingDate` within ±7 days of `publishedAt` | Geographic grounding matches localized answer engines |
| `topic_relevance` | `topicDetails.topicCategories` vs target keyword (C2, api only) | last URL segment, `_`→space; `Jaccard(keyword tokens) × 100` | Topic taxonomy matches query intent |

### 7.5 Composite
```
total = (seo_weight * seo_total + geo_weight * geo_total) / (seo_weight + geo_weight)
seo_total = Σ(w_i * comp_i) / Σ w_i ; geo_total likewise
```
Weights: `TUBEFORGE_WEIGHTS_SEO`, `TUBEFORGE_WEIGHTS_GEO`, `TUBEFORGE_SEO_*`, `TUBEFORGE_GEO_*` env keys; defaults baked (each component set sums 1.0 — SEO set now 18, GEO set 7); C1/C2 signals via `TUBEFORGE_GEO_LOCATION_SIGNAL` / `TUBEFORGE_GEO_TOPIC_RELEVANCE`; `settings_json` overrides.

### 7.6 Output (persisted `scores.components`)
```json
{ "keyword_title": 82, "title_front": 100, "title_length": 90,
  "keyword_desc": 55, "desc_first150": 100, "tags_relevance": 74,
  "entity_coverage": 80, "qa_phrasing": 60, "metadata_complete": 90 }
```

---

## 8. Analytics Modules

### 8.1 Graph (`analytics/graph.rs`)
- Build adjacency from `edges` (competitor overlap: co-occurring keywords in titles/tags auto-suggested, weight = overlap strength) + manual edges.
- **PageRank** (damped 0.85, 50 iterations — converges trivially at this scale) → centrality per channel; persisted in `meta` cache.
- Outputs: scorecard `influence` axis, ideas `competitor gap` signal.

### 8.2 Next Ideas (`analytics/ideas.rs`)
- Candidate generation: high-scoring competitor titles (top BM25 neighborhoods) + keyword list.
- Rank = `0.5*seo_total + 0.3*idea_fit + 0.2*competitor_gap` (idea_fit = similarity to user niche/keywords; competitor_gap = low centrality competitor in high-demand keyword).
- Persist `ideas` with rationale JSON; `ideas --status saved` marks.

### 8.3 Keyword Rank Tracking (`analytics/keywords.rs`)
- `keywords check`: for each keyword, BM25 over corpus → top video + position → snapshot into `keyword_rankings` (position NULL when below threshold); snapshots carry `topics` JSON (topic categories at check time, migration 002).
- Ranks report: trend per keyword across snapshots (CLI table; deltas computed in Rust — no SQL window functions).

### 8.4 Reports (`analytics/reports.rs`)
- **scorecard:** per channel vs median of competitors: views growth proxy (rss views), title patterns, tag overlap, PageRank centrality, Louvain community, SEO score distribution. `--json` for agents.
- **health:** rows counts, last ingest, quota state, integrity re-open result, stale channels (>N days), index freshness, `privacy` census.
- **alerts:** rules — quota exhausted, integrity failure, brand keyword absent from competitor top titles, stale channel, new competitor detected, `video_unavailable`.

### 8.5 Growth & Content (`analytics/growth.rs`, `content.rs`, `forecast.rs`)
- **`growth.rs`:** own-channel overview (`own_overview`), keyword opportunities (forecast-ranked chart-ready series), next-video recommendations (`next_video_recommendations` — excludes covered topics, auto-drafts packaging, view-prediction tier + "why"). Consumed by `analysis.*` RPC + API.
- **`content.rs`:** deterministic `generate(DraftInput) -> ContentDraft` — keyword-first title (≤55 chars, Title Doctrine, no emoji), SEO-shaped description, ≤12 tags.
- **`forecast.rs`:** weighted OLS on elapsed time with recency half-life (30d); `MIN_POINTS=3`; t-stat ±2.0 significance gate; `TREND_THRESHOLD_PCT=10%` → Rising/Flat/Falling; LOW/MEDIUM/HIGH reliability; `next_estimate`, `slope_per_day`, `r_squared`. Fed by `channel_snapshots`.

### 8.6 Knowledge Graph (`kg_builder.rs`, `kg_algorithms.rs`, `graph_aware.rs`)
- **Build:** reads videos/channels/keywords/edges/keyword_rankings → 6 entity types (Video, Channel, Tag, Keyword, Topic, Entity), 9 relation types (tags, created_by, about_topic, competes_in, dominates, related_to, similar_to, mentioned_in, contains). Weighted edges (tag `1/(1+pos)`, keyword `1/(1+position)`); Jaccard tag co-occurrence ≥2. `BuildMode::Full` (clears KG tables) or `Incremental`.
- **Algorithms:** Louvain community detection + PageRank centrality (`kg_algorithms.rs`).
- **Load:** `load_or_build()` checks `meta.kg_cache_json`; on miss full-rebuilds then `load_from_db`. Lazy-loaded in `serve.rs` via double-checked locking, cached for server lifetime; graceful degradation to `null`.
- **Consumers:** `graph_aware::compute_graph_scores` (tag_authority/topic_dominance/keyword_competition), `generate_graph_ideas`, `find_content_gaps`, `compute_tag_authority_by_name`; `graph.rs` lightweight channel-graph helpers (`tag_authority_scores`, `topic_dominance_scores`, `build_kw_channel_graph`).

---

## 9. Backup, Recovery & Migration

### 9.1 Backup (locked policy)
```
backup:
  copy <db>.dat → backups/tubeforge-<ts>.db     -- standalone tfdb checkpoint (self-contained)
  reopen the copy with Db::open (integrity)     → exit 5 on failure
  prune: keep last N (TUBEFORGE_BACKUP_KEEP, default 10)
```
- Auto-run before every batch ingest (guard). BM25 index NOT backed up (rebuild via `reindex`).

### 9.2 Recovery
- Restore: point `TUBEFORGE_DB_PATH` at backup or copy over main; then `reindex`.
- **No SQLite escape hatch** (ADR-1) — backup checkpoints are self-contained `tfdb` snapshots; integrity verified by re-open.

### 9.3 Migrations
- `meta.schema_version`; ordered migration list in `storage/schema.rs`; each migration runs in one transaction; version bump persisted. `init`/open applies pending migrations. **SCHEMA_VERSION = 9.**
- Migration 001 (full v1 schema) is **idempotent and always applied** — Phase 0-era DBs gain the full schema in place without a version bump (their recorded v1 is retained).
- Migration 002 (Phase 2, 1→2): adds recording-date/location/lat/lng + `topic_categories` (JSON) and `keyword_rankings.topics` (JSON); version-gated.
- Migration 003 (Phase 3, 2→3): adds `videos.privacy_status` TEXT; version-gated.
- Migrations 004–009 (Phase 6): transcripts/comments/video_heatmap/channel_snapshots/keyword_research + kg_entities/kg_relations/kg_communities + tag tables → **SCHEMA_VERSION 9**; version-gated idempotent.

---

## 10. Concurrency & Async Model

- **tokio** runtime (network). Storage calls are **synchronous** (`tfdb` engine is sync; the `Db` facade exposes async for legacy call-site parity). Issue blocking engine work via `spawn_blocking` or between await points; never hold a DB guard across an await.
- Strictly **one writer** at a time: `tfdb` serializes writers with a single write lock; CLI process model makes this trivially true; no daemon. `serve` (dashboard) holds one shared Db — see §4.5 single-writer caveat (never run it concurrently with writing CLI commands; snapshot/in-memory readers fine).
- Retries (network only): 3× exponential backoff with jitter; idempotent by design (PK upserts).
- Cancellation: Ctrl-C → abort between phases; transaction rollback protects; next run re-enters safely (ingest_log + PKs).

---

## 11. Configuration (`.env`)

| Key | Default | Purpose |
|---|---|---|
| `YOUTUBE_API_KEY` | *(empty)* | Optional API key; empty = RSS/oEmbed only |
| `TUBEFORGE_DB_PATH` | `~/.tubeforge/tubeforge.db` | DB base path → produces `<path>.wal` + `<path>.dat` |
| `TUBEFORGE_DATA_DIR` | `~/.tubeforge/` | index/, backups/ root |
| `TUBEFORGE_BACKUP_DIR` | `<data>/backups` | Snapshot location |
| `TUBEFORGE_BACKUP_KEEP` | `10` | Retention |
| `TUBEFORGE_WEIGHTS_SEO` | `1.0` | SEO/GEO composite weights |
| `TUBEFORGE_WEIGHTS_GEO` | `1.0` | ↑ |
| `TUBEFORGE_SEO_*` / `TUBEFORGE_GEO_*` | defaults | Per-component weights |
| `TUBEFORGE_GEO_LOCATION_SIGNAL` | `0.10` | `location_signal` GEO weight (C1) |
| `TUBEFORGE_GEO_TOPIC_RELEVANCE` | `0.10` | `topic_relevance` GEO weight (C2) |
| `TUBEFORGE_QUOTA_WARN_AT` | `90` (percent) | Warn threshold |
| `TUBEFORGE_CHROMIUM_DIR` | `<data>/chromium` | Headless Chromium install root (chromiumoxide_fetcher-pinned, auto-downloaded) |
| `TUBEFORGE_FILMOT_KEY` | *(empty)* | Opt-in Filmot API key for `filmot get`; empty = command disabled (CONFIG error) |
| `TUBEFORGE_SERVE_PORT` | `8080` | `serve` listen port (flag `--port` overrides; must be a number — else CONFIG error) |
| `LOG_LEVEL` | `info` | tracing filter |

---

## 12. Testing Strategy

**Current: 165/165 tests passing + 2 ignored (Aug 4, 2026)** — the 2 ignored: the opt-in performance gate + the Chromium-gated render. Suites: `tests/agent_contract.rs` (11 binary-level `--json` contract tests), `tests/serve.rs` (12 dashboard HTTP tests), and `tests/perf_gate.rs` (2 sanity tests + 1 ignored gate).

| Layer | Tests |
|---|---|
| Unit | ID extraction regexes, scoring formulas (golden vectors), quota math, weight parsing, PageRank on toy graphs, JSON envelope shape, **ID checksum tables (bare video/channel regexes), extended URL-form parsing (`/v/ /embed/ /shorts/ /video/ /watch/ /live/`, playlist prefixes, `@handle`/`/user/`/`/c/`), `SC→UC` channel transform, category lookup (32 ids), disabled-vs-unknown metric heuristic, location/topic golden vectors, thumbnail template fill, asset-cleanup RAII (temp dir removed on success + error path), CSV escaping (quotes/commas/newlines), Filmot tolerant parse (missing fields/keys)** |
| Integration (wiremock fixture server) | RSS parse from fixture feed; oEmbed; API batching ≤50 ids; ETag 304 path; quota 403 → fallback |
| Storage (`tfdb`) | upsert idempotency, source precedence, WAL replay + torn-tail truncation, atomic checkpoint, fsync-on-commit durability, migration idempotency (version-gated re-run), **backup round-trip: ingest → backup → restore → integrity re-open == ok**, property tests (dedupe fuzz, ingest idempotency) |
| Compatibility | **no SQLite escape hatch** (ADR-1) — removed; backup round-trip integrity via `Db::open` re-open on the snapshot copy |
| Agent contract (`tests/agent_contract.rs`, binary-level) | 11 tests: every command's `--json` → single JSON object on stdout only; no ANSI codes; tracing on stderr only; envelope `ok/data/meta` + `error` shapes |
| Property | dedupe fuzz (random URL sets), ingest idempotency (run twice → same state) |
| Performance smoke | 5k videos ingest + reindex + top-k ideas < 30s on M4 (gate) — **✅ PASSED (Aug 4, 2026, M4, release profile)**: ingest 20.22s / reindex 0.20s / ideas 0.06s / total 20.49s vs 30s budget (per-phase budgets ingest<25 reindex<10 ideas<5). Run: `cargo test --release --test perf_gate -- --ignored --nocapture`. Scaling finding: post-ingest scoring is the dominant cost, ~4ms/video at 5k corpus (`tests/perf_gate.rs`) |
| Render (ignored, env-gated) | `thumbnail render` end-to-end vs headless Chromium — 1 ignored test (requires pinned Chromium download) |
| Template autoescaping (askama) | dashboard templates render without injecting raw HTML; user-controlled values (titles, alerts, ideas) escaped by askama 0.14 at compile time |
| SVG escaping | chart rendering (`serve/svg.rs`) — values/axis labels escaped; no HTML injection through chart input |
| CSRF policy (`tests/serve.rs`) | bad Origin → 403; absent Origin/Referer → allowed (curl/agents); mismatched Referer → 403 |
| Serve integration (`tests/serve.rs`) | ephemeral-port server + real Db: all pages respond 200, `/healthz` plain "ok", unknown route 404, `static/htmx.min.js` served, **DB-verified mutations** (idea status POST flips row, alerts read/clear POSTs mutate rows), home embeds counts + charts, score detail fragment lists all 17 components |

---

## 13. Phase 0 Gate (superseded — records the v3 stack)

**STATUS: ✅ PASSED — August 3, 2026 (turso `=0.7.2`, tantivy `=0.26.1`, macOS arm64/M4). Superseded by the v4.0 engine-independence re-architecture (ADR-1/2): Turso/SQLite and tantivy were replaced by `tfdb` + own BM25 (Phase 6, Aug 14 2026). Kept for provenance.**

| # | Item | Result |
|---|---|---|
| 1 | turso pinned; CRUD + WAL + transaction + `integrity_check` | ✅ PASS |
| 2 | `VACUUM INTO` + restore round-trip passes `integrity_check` | ✅ PASS (25/25 rows; retention prune verified) |
| 3 | FTS probe (`fts_match`/`fts_score`) | ✅ CONFIRMED — `CREATE INDEX … USING fts` is a **hard syntax error** in 0.7.2. Engine FTS unavailable → tantivy-direct (now own-BM25) confirmed as the only path |
| 4 | rusqlite opens the same `.db` | ✅ PASS (v3 stack) — removed in v4.0 |
| 5 | CLI skeleton `init`/`ingest links`/`backup`/`quota`, JSON envelope + exit codes | ✅ PASS (exit 0/1/2 verified; `--json` envelope shapes verified) |

**Current storage engine (`tfdb`, v4.0):**
- File layout: `<path>.wal` (append-only, CRC32-checksummed transaction log) + `<path>.dat` (latest atomic checkpoint snapshot). Magics `TFWL`/`TFDT`; rows serialized via deterministic serde_json.
- Durability: `begin`→`Tx` stages in memory; `commit` writes one WAL record + `fsync` (default on) then applies to the live snapshot; crash replays WAL and truncates any torn tail; `checkpoint()` writes the full snapshot via temp-file + `rename` (atomic) then truncates the WAL.
- `EngineOptions.fsync_on_commit` (default true) controls the fsync; single write lock serializes writers.

---

## 14. Open Items (LLD level)

1. ~~SEO/GEO weights & formula final values (needs user's scoring spec — PRD §5.2).~~ → **Resolved (Aug 4, 2026):** documented defaults baked in (10 SEO + 7 GEO components, each set sums 1.0); tunable via `TUBEFORGE_SEO_*` / `TUBEFORGE_GEO_*`.
2. ~~tantivy + turso exact version pins~~ → **Resolved at gate (v3 stack); superseded by v4.0 engine-independence (ADR-1/2).** Current pins: tokio `=1.53.1`, hyper `1.11.0`, askama `0.14`, chromiumoxide `0.9.1` + fetcher, zip `8.6`, rustc 1.85+ (`Cargo.toml` rust-version).
3. ~~Thumbnail HTML→image method (SVG+resvg vs headless Chromium)~~ → **Resolved (Aug 4, 2026):** headless Chromium via **chromiumoxide 0.9.1** (CDP) + chromiumoxide_fetcher-pinned Chromium into `<data>/chromium` (rustls, no native-tls); Tailwind v4 compiled via standalone CLI (no Node); rationale — literal HTML+Tailwind v4, Blink determinism, pinned browser (no system Chrome dependency), permissive licensing.
4. Embedding strategy post-v1 (lexical-only in v1, ADR-9). HNSW module ships (`src/tfdb/hnsw.rs`) but is unwired — no embedding pipeline; wire + generate embeddings post-release.
5. ~~Windows CI target timing (post-macOS release)~~ → **Resolved (Aug 4, 2026):** Windows CI added to the Phase 4 workflow matrix (`windows-latest`); cross-platform test results will come from the first CI run once the repo is pushed.
6. ~~Undocumented YouTube API limits (`search.list` ~750-result cap; `playlistItems.list` 20k-video cap)~~ → **Resolved (Aug 4, 2026):** documented from MW Metadata wiki research — see §5.3 API behavior notes; RSS `feeds/videos.xml` has no playlist cap.
