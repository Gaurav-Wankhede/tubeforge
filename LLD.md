# TubeForge — Low-Level Design (LLD)

**Project:** TubeForge — local-first YouTube SEO/GEO growth engine
**Document version:** 1.3 | **Date:** August 4, 2026
**Status:** Approved — Phases 0–3 delivered; implementation reference for Phase 4+
**Companion documents:** `PRD.md` (v3.12), `HLD.md`

---

## 1. Scope

This document specifies: module layout, data model (SQL schema + tantivy index spec), CLI contracts (commands, JSON envelope, exit codes, error taxonomy), fetch-layer design (RSS/oEmbed/API + quota), ingest pipeline semantics, scoring engine formulas, analytics modules, backup/recovery flows, concurrency model, configuration keys, migration/versioning, and the testing strategy.

**Grounding constraints (locked, evidence-backed):**
- Engine: Turso Database, **WAL mode only**, pinned release. No Turso FTS/vector index modules. (HLD §7, §11)
- BM25: tantivy crate, owned by TubeForge. Vector: brute-force cosine in Rust. Graph: Rust PageRank.
- Backup before every batch ingest; rusqlite escape hatch.
- CLI-only v1; `--json` envelope; external MCP via `tursodb --mcp`.

---

## 2. Module Layout (single crate, phase-gated)

```
tubeforge/
├── Cargo.toml                  # bin `tubeforge`; deps: turso, tantivy, tokio, clap,
│                               #   reqwest (rustls, NOT native-tls), quick-xml, serde,
│                               #   dotenvy, tracing, chrono, chrono-tz, url, thiserror
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
│   │   └── quota.rs            # per-endpoint budget ledger (persisted in meta table)
│   ├── ingest.rs               # resolution, ID extraction, dedupe, upsert orchestration
│   ├── categories.rs           # YouTube category map (32 ids → names)
│   ├── storage/
│   │   ├── mod.rs              # Db trait (Turso impl + rusqlite impl behind feature)
│   │   ├── db.rs               # Turso connection, repository methods
│   │   ├── schema.rs           # embedded schema.sql + migrations
│   │   └── backup.rs           # VACUUM INTO, integrity_check, retention, restore
│   ├── search/
│   │   ├── mod.rs
│   │   ├── index.rs            # tantivy IndexWriter/Reader lifecycle, rebuild
│   │   └── bm25.rs             # query construction, score retrieval
│   ├── scoring/
│   │   ├── mod.rs
│   │   ├── seo.rs              # structural + BM25-derived SEO components
│   │   ├── geo.rs              # free-signal GEO components
│   │   └── weights.rs          # weight config (defaults + .env overrides)
│   ├── analytics/
│   │   ├── mod.rs
│   │   ├── graph.rs            # competitor adjacency + PageRank
│   │   ├── ideas.rs            # Next Ideas generation + ranking
│   │   ├── keywords.rs         # rank tracking snapshots
│   │   ├── reports.rs          # scorecard, health, alerts
│   ├── thumbnail/
│   │   ├── mod.rs              # template fill, render orchestration (chromiumoxide)
│   │   ├── render.rs           # CDP render → PNG 1280×720
│   │   └── assets.rs           # per-render temp dir + RAII cleanup guard
│   ├── export/
│   │   ├── mod.rs              # manifest.json + JSON arrays
│   │   └── csv.rs              # videos/channels/tags/keywords CSV writers (escaping)
│   ├── templates/
│   │   ├── default.html input.css tailwind.css   # compiled Tailwind v4 CSS committed
│   └── commands/               # one file per CLI subcommand
│       ├── init.rs ingest.rs score.rs ideas.rs keywords.rs
│       ├── scorecard.rs health.rs alerts.rs backup.rs quota.rs reindex.rs
│       ├── thumbnail.rs availability.rs export.rs filmot.rs
└── tests/
    ├── fixtures/               # local HTTP server (wiremock) RSS/oEmbed/API payloads
    └── *.rs                    # integration + property tests
```

**Dependency rule:** `commands` → domain modules → `storage`/`search` (leaf). `storage` is the only module importing `turso`; `search` is the only module importing `tantivy`.

---

## 3. Data Model

### 3.1 SQL schema (Turso, WAL mode)

```sql
PRAGMA journal_mode = WAL;
PRAGMA user_version = 3;   -- managed by migrations (SCHEMA_VERSION = 3)

CREATE TABLE channels (
  channel_id        TEXT PRIMARY KEY,      -- UC...  (or handle-resolved id)
  handle            TEXT UNIQUE,           -- @name
  title             TEXT NOT NULL,
  description       TEXT,
  avatar_url        TEXT,
  country           TEXT,
  subscriber_count  INTEGER,               -- api only
  video_count       INTEGER,               -- api only
  source            TEXT NOT NULL DEFAULT 'rss',  -- rss | api
  etag              TEXT,                  -- rss caching
  fetched_at        TEXT NOT NULL,         -- ISO8601 UTC
  updated_at        TEXT NOT NULL
);

CREATE TABLE videos (
  video_id      TEXT PRIMARY KEY,          -- 11-char id
  channel_id    TEXT REFERENCES channels(channel_id) ON DELETE SET NULL,
                                            -- NULLABLE: oEmbed-sourced links have
                                            -- no channel_id; store @handle-keyed
                                            -- placeholder channel when known
  title         TEXT NOT NULL,
  description   TEXT NOT NULL DEFAULT '',
  tags          TEXT NOT NULL DEFAULT '[]',   -- JSON array (api only)
  category_id   TEXT,
  duration_sec  INTEGER,
  published_at  TEXT NOT NULL,             -- ISO8601 UTC
  view_count    INTEGER,
  like_count    INTEGER,
  comment_count INTEGER,
  recording_date        TEXT,              -- api only (recordingDetails.date)
  recording_location_name TEXT,            -- api only (recordingDetails.location)
  recording_lat         REAL,              -- api only
  recording_lng         REAL,              -- api only
  topic_categories      TEXT NOT NULL DEFAULT '[]',  -- JSON array (api only, topicDetails)
  thumb_url     TEXT,
  embedding     BLOB,                      -- RESERVED: semantic embeddings;
                                           --   unused in v1 (lexical-only), so
                                           --   adding them later = no migration
  source        TEXT NOT NULL DEFAULT 'rss',  -- rss | oembed | api
  privacy_status TEXT,                        -- public | unlisted | private (api only, migration 003)
  fetched_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);
CREATE INDEX idx_videos_channel      ON videos(channel_id);
CREATE INDEX idx_videos_published    ON videos(published_at DESC);
CREATE INDEX idx_videos_channel_pub  ON videos(channel_id, published_at DESC);
CREATE TABLE competitors (
  channel_id  TEXT PRIMARY KEY REFERENCES channels(channel_id) ON DELETE CASCADE,
  label       TEXT,                          -- display name / grouping
  added_at    TEXT NOT NULL
);

CREATE TABLE keywords (
  keyword     TEXT PRIMARY KEY,
  niche       TEXT,
  created_at  TEXT NOT NULL
);

CREATE TABLE keyword_rankings (              -- snapshot per check
  keyword     TEXT NOT NULL REFERENCES keywords(keyword) ON DELETE CASCADE,
  checked_at  TEXT NOT NULL,
  video_id    TEXT REFERENCES videos(video_id) ON DELETE SET NULL,
  position    INTEGER,                       -- NULL = not found
  topics      TEXT,                          -- JSON (api only): topic categories at check time
  PRIMARY KEY (keyword, checked_at)
);

CREATE TABLE scores (
  video_id     TEXT PRIMARY KEY REFERENCES videos(video_id) ON DELETE CASCADE,
  seo_score    REAL NOT NULL,                -- 0..100
  geo_score    REAL NOT NULL,                -- 0..100
  total_score  REAL NOT NULL,                -- weighted composite 0..100
  components   TEXT NOT NULL,                -- JSON breakdown (per-signal values)
  computed_at  TEXT NOT NULL
);
CREATE INDEX idx_scores_total ON scores(total_score DESC);

CREATE TABLE ideas (
  idea_id        INTEGER PRIMARY KEY AUTOINCREMENT,
  title_suggestion TEXT NOT NULL,
  rationale      TEXT NOT NULL,              -- JSON: signals that fired
  score          REAL NOT NULL,
  status         TEXT NOT NULL DEFAULT 'draft',  -- draft | saved | discarded
  source_video   TEXT REFERENCES videos(video_id) ON DELETE SET NULL,
  created_at     TEXT NOT NULL
);

CREATE TABLE edges (                          -- competitor graph
  from_channel TEXT NOT NULL REFERENCES channels(channel_id) ON DELETE CASCADE,
  to_channel   TEXT NOT NULL REFERENCES channels(channel_id) ON DELETE CASCADE,
  weight       REAL NOT NULL DEFAULT 1.0,
  source       TEXT NOT NULL,                -- overlap | manual
  PRIMARY KEY (from_channel, to_channel)
);

CREATE TABLE alerts (
  alert_id   INTEGER PRIMARY KEY AUTOINCREMENT,
  kind       TEXT NOT NULL,                  -- brand | gap | quota | integrity
  channel_id TEXT REFERENCES channels(channel_id) ON DELETE CASCADE,
  message    TEXT NOT NULL,
  severity   TEXT NOT NULL DEFAULT 'info',   -- info | warn | critical
  created_at TEXT NOT NULL,
  read_at    TEXT
);

CREATE TABLE ingest_log (
  batch_id   TEXT NOT NULL,
  item       TEXT NOT NULL,                  -- channel id / video id / url
  status     TEXT NOT NULL,                  -- ok | skipped | failed
  detail     TEXT,
  at         TEXT NOT NULL
);
CREATE INDEX idx_ingest_log_batch ON ingest_log(batch_id);

CREATE TABLE meta (                           -- key/value store
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
-- keys: schema_version, quota_videos_list_used, quota_videos_list_date,
--       last_backup_at, last_reindex_at, settings_json
```

### 3.2 tantivy index spec

- **Location:** `<data>/index/` (rebuildable — not part of backups).
- **Schema fields:**
  - `video_id` (STRING, STORED)
  - `channel_id` (STRING, STORED)
  - `title` (TEXT, indexed, tokenized, stored)
  - `description` (TEXT, indexed, tokenized)
  - `tags` (TEXT, indexed, tokenized — joined)
  - `published_at` (DATE, indexed)
- **Query surface (bm25.rs):**
  - `score_title(q)` — BM25 over `title`
  - `score_desc(q)` — BM25 over `description`
  - `score_tags(q)` — BM25 over `tags`
  - `top_similar(video_id, n)` — combined field query, exclude self
- **Lifecycle:** `IndexWriter` per ingest batch (add/delete by `video_id`), commit; `Reader` reload for scoring queries. Full rebuild = truncate dir + re-index from `videos` table (idempotent — recovery path for any index inconsistency).

### 3.3 Rationale notes

- **No FTS virtual table** — Turso's FTS5 `MATCH` is unsupported and Turso FTS is beta (HLD §7.2). tantivy owns all text ranking.
- **Embeddings column: reserved, unused in v1.** Cosine similarity operates on token-overlap vectors (title/tags) — *lexical* similarity (ADR-9, user-locked Aug 3 2026). Semantic embeddings can be added later **without any schema change** (the `embedding` BLOB column already exists in the schema).
- **`meta` schema_version** drives migrations (section 9). **SCHEMA_VERSION = 3** (migration 003, Phase 3): adds `videos.privacy_status` (public/unlisted/private, api only); version-gated idempotent, 001/002 unchanged.
- **Ingest idempotency:** `video_id` PK → upsert semantics (see 5.3).

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
| `reindex` | Rebuild tantivy from `videos` | — |
| `thumbnail render` | Render template → 1280×720 PNG; raw assets in per-render temp dir, deleted after success (RAII guard; `--keep-assets` debug-only) | `--template`, `--title`, `--output`, `--keep-assets`, `--json` |
| `thumbnail list-templates` | List available HTML+Tailwind templates | `--json` |
| `check availability` | Batched `videos.list` (part=snippet,status); missing IDs → `video_unavailable` alerts; records `privacy_status` | `--json` |
| `export` | Export DB to `--format zip\|dir`: manifest.json, videos.csv (19 cols), channels.csv, tags.csv, keywords.csv, keyword_rankings.csv + JSON arrays (videos, ideas, alerts, scores); deterministic zip archives | `--format`, `--output`, `--json` |
| `filmot get` | Opt-in recovery lookup via Filmot API (`TUBEFORGE_FILMOT_KEY`); raw JSON passthrough + tolerant summary; no DB writes; non-fatal. Empty key → exit 1 CONFIG error | `--video-id`, `--json` |
| `serve` | **Deferred** (HTMX dashboard) | — |

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
  Index { detail: String },                   // tantivy errors
  Usage(String),                              // → exit 2
}
```
Mapping: `From<TubeforgeError> for i32` centralizes exit codes. Errors always render in the JSON envelope under `error`, and human mode prints `code: message` on stderr.

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

### 6.3 Transaction + ordering rules (Turso constraint)
- **One write statement active per connection** (Turso returns `SQLITE_BUSY` otherwise — COMPAT.md). Pipeline is strictly sequential: fetch-all → single transaction → single writer. Reads between writes are fine.
- Entire batch in one transaction; on any failure → rollback, log failed items to `ingest_log`, exit 1.
- **Backup guard:** automatic `backup` (VACUUM INTO + integrity_check) before every batch write unless `--no-backup`. Integrity failure → abort with exit 5 (never write into a corrupt DB).

### 6.4 Post-ingest
- tantivy: delete stale docs for updated videos + add new (one writer commit).
- Scoring: recompute `scores` only for changed/inserted videos.
- `ingest_log` rows per item; summary counts → output.

---

## 7. Scoring Engine

### 7.1 Pipeline
```
Inputs: title, description, tags[], channel context, target keywords (optional)
Signals → components → weighted total (0–100) → components JSON persisted
```
> **Phase 2 status (Aug 4, 2026):** full engine delivered — 10 SEO + 7 GEO components (incl. C1/C2 `location_signal` / `topic_relevance`), k-scaled formulas, baked defaults + env overrides; Phase 1 BASIC mode superseded.

### 7.2 SEO components (default weights; override via env)

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

### 7.3 GEO components (free signals only — no paid API)

| Component | Signal | Formula sketch (v1, deterministic) | Rationale |
|---|---|---|---|
| `entity_coverage` | who/what/when/where/how present in desc | `n_present/5 × 100` | AI answer engines cite complete entities |
| `qa_phrasing` | question-style headings ("What is…?") | question-form heading → 100/60/0 | Answer-shaped content gets cited |
| `list_phrasing` | lists/bullets/step markers | checklist: bullets/lists/steps present | Extractable answers |
| `conversational` | natural tone markers vs keyword-stuffing (density ceiling) | density past ceiling → linear penalty | Over-optimization penalty |
| `metadata_complete` | desc, tags, timestamps present | checklist score (0–100) | Structured completeness |
| `location_signal` | `recordingDetails` lat/lng or location name (C1, api only) | `70` when lat/lng or location name present; `+30` when `recordingDate` within ±7 days of `publishedAt` | Geographic grounding matches localized answer engines |
| `topic_relevance` | `topicDetails.topicCategories` vs target keyword (C2, api only) | last URL segment, `_`→space; `Jaccard(keyword tokens) × 100` | Topic taxonomy matches query intent |

### 7.4 Composite
```
total = (seo_weight * seo_total + geo_weight * geo_total) / (seo_weight + geo_weight)
seo_total = Σ(w_i * comp_i) / Σ w_i ; geo_total likewise
```
Weights: `TUBEFORGE_WEIGHTS_SEO`, `TUBEFORGE_WEIGHTS_GEO`, `TUBEFORGE_SEO_*`, `TUBEFORGE_GEO_*` env keys; defaults baked (each component set sums 1.0 — 10 SEO, 7 GEO); new C1/C2 signals via `TUBEFORGE_GEO_LOCATION_SIGNAL` / `TUBEFORGE_GEO_TOPIC_RELEVANCE`; `settings_json` overrides.

### 7.5 Output (persisted `scores.components`)
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
- Ranks report: trend per keyword across snapshots (CLI table; `lag/lead` unavailable in Turso — compute deltas in Rust).

### 8.4 Reports (`analytics/reports.rs`)
- **scorecard:** per channel vs median of competitors: views growth proxy (rss views), title patterns, tag overlap, centrality, SEO score distribution. `--json` for agents.
- **health:** rows counts, last ingest, quota state, integrity_check result, stale channels (>N days), index freshness.
- **alerts:** rules — quota exhausted, integrity failure, brand keyword absent from competitor top titles, stale channel, new competitor detected.

---

## 9. Backup, Recovery & Migration

### 9.1 Backup (locked policy)
```
backup:
  VACUUM INTO backups/tubeforge-<ts>.db      -- consistent snapshot, single file
  PRAGMA integrity_check → exit 5 on failure
  prune: keep last N (TUBEFORGE_BACKUP_KEEP, default 10)
```
- Auto-run before every batch ingest (guard). tantivy index NOT backed up (rebuild via `reindex`).

### 9.2 Recovery
- Restore: point `TUBEFORGE_DB_PATH` at backup or copy over main; then `reindex`.
- Escape hatch: same `.db` opens in rusqlite/SQLite (COMPAT guarantee #1) — CI test enforces.

### 9.3 Migrations
- `meta.schema_version` (mirrors `PRAGMA user_version`); ordered migration list in `storage/schema.rs`; each migration runs in one transaction; version bump persisted. `init`/open applies pending migrations.
- Migration 001 (full v1 schema) is marked **idempotent and always applied** — Phase 0-era DBs (meta-only) gain the full schema in place without a version bump (their recorded v1 is retained). This is the documented Phase 0→v1 upgrade path.
- Migration 002 (Phase 2, SCHEMA_VERSION 1→2): adds `videos.recording_date` / `recording_location_name` / `recording_lat` / `recording_lng` / `topic_categories` (JSON) and `keyword_rankings.topics` (JSON); idempotent via version gating — 001 untouched, 002 never re-runs after the bump.
- Migration 003 (Phase 3, SCHEMA_VERSION 2→3): adds `videos.privacy_status` TEXT (public/unlisted/private, api only, from `check availability`); idempotent via version gating — 001/002 untouched, 003 never re-runs after the bump.
- Rule: migrations never depend on experimental Turso features.

---

## 10. Concurrency & Async Model

- **tokio** runtime (network). Storage calls are **synchronous** (Turso crate): issue from `spawn_blocking` or between await points; never hold a DB guard across an await. (Turso API is async — use its async API directly; sequential awaits, single writer.)
- Strictly **one writer** at a time: CLI process model makes this trivially true; no daemon.
- Retries (network only): 3× exponential backoff with jitter; idempotent by design (PK upserts).
- Cancellation: Ctrl-C → abort between phases; transaction rollback protects; next run re-enters safely (ingest_log + PKs).

---

## 11. Configuration (`.env`)

| Key | Default | Purpose |
|---|---|---|
| `YOUTUBE_API_KEY` | *(empty)* | Optional API key; empty = RSS/oEmbed only |
| `TUBEFORGE_DB_PATH` | `~/.tubeforge/tubeforge.db` | DB file |
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
| `LOG_LEVEL` | `info` | tracing filter |

---

## 12. Testing Strategy

**Current: 135/135 tests passing + 1 ignored (Chromium-gated render) (Aug 4, 2026)**.

| Layer | Tests |
|---|---|
| Unit | ID extraction regexes, scoring formulas (golden vectors), quota math, weight parsing, PageRank on toy graphs, JSON envelope shape, **ID checksum tables (bare video/channel regexes), extended URL-form parsing (`/v/ /embed/ /shorts/ /video/ /watch/ /live/`, playlist prefixes, `@handle`/`/user/`/`/c/`), `SC→UC` channel transform, category lookup (32 ids), disabled-vs-unknown metric heuristic, location/topic golden vectors, thumbnail template fill, asset-cleanup RAII (temp dir removed on success + error path), CSV escaping (quotes/commas/newlines), Filmot tolerant parse (missing fields/keys)** |
| Integration (wiremock fixture server) | RSS parse from fixture feed; oEmbed; API batching ≤50 ids; ETag 304 path; quota 403 → fallback |
| Storage | upsert idempotency, source precedence, migration v0→v1, **v1→v2 idempotency (version-gated re-run)**, **v2→v3 idempotency (privacy_status, version-gated re-run)**, **backup round-trip: ingest → backup → restore → integrity_check == ok** |
| Compatibility | open `.db` via rusqlite (escape hatch) — CI every build |
| Agent contract (`tests/agent_contract.rs`, binary-level) | 11 tests: every command's `--json` → single JSON object on stdout only; no ANSI codes; tracing on stderr only; envelope `ok/data/meta` + `error` shapes |
| Property | dedupe fuzz (random URL sets), ingest idempotency (run twice → same state) |
| Performance smoke | 5k videos ingest + reindex + top-k ideas < 30s on M4 (gate) |
| Render (ignored, env-gated) | `thumbnail render` end-to-end vs headless Chromium — 1 ignored test (requires pinned Chromium download) |

---

## 13. Phase 0 Gate (must pass before feature work)

**STATUS: ✅ PASSED — August 3, 2026 (turso `=0.7.2`, tantivy `=0.26.1`, macOS arm64/M4)**

| # | Item | Result |
|---|---|---|
| 1 | turso pinned; CRUD + WAL + transaction + `integrity_check` | ✅ PASS |
| 2 | `VACUUM INTO` + restore round-trip passes `integrity_check` | ✅ PASS (25/25 rows; retention prune verified) |
| 3 | FTS probe (`fts_match`/`fts_score`) | ✅ CONFIRMED — `CREATE INDEX … USING fts` is a **hard syntax error** in 0.7.2 (not merely wrong ranking). Engine FTS unavailable → tantivy-direct confirmed as the only path |
| 4 | rusqlite opens the same `.db` | ✅ PASS — main WAL-mode db opened directly (no snapshot fallback needed) |
| 5 | CLI skeleton `init`/`ingest links`/`backup`/`quota`, JSON envelope + exit codes | ✅ PASS (exit 0/1/2 verified; `--json` envelope shapes verified) |

**Engine API notes (turso 0.7.2 — recorded for Phase 1+):**
- `Builder::new_local(path).experimental_vacuum(true).experimental_index_method(true).build().await` → `connect()` → `Connection`. **`VACUUM INTO` requires `experimental_vacuum(true)`.**
- Params: heterogeneous tuples via the `params!` macro work (Phase 0 note that only homogeneous `Vec<T: IntoValue>` was supported is **outdated** — verified in Phase 1); transactions: `transaction()` + `commit()`/`rollback()` (no savepoints).
- Row-returning PRAGMAs (`journal_mode`, `user_version` reads) must go through `query`, not `execute`; the SET form `PRAGMA user_version = 1` returns no rows and must be `execute`d.
- Backups: turso creates an (empty) `-wal` companion when opening snapshots; the `.db` alone remains complete and portable (rusqlite-verified standalone).

---

## 14. Open Items (LLD level)

1. ~~SEO/GEO weights & formula final values (needs user's scoring spec — PRD §5.2).~~ → **Resolved (Aug 4, 2026):** documented defaults baked in (10 SEO + 7 GEO components, each set sums 1.0); tunable via `TUBEFORGE_SEO_*` / `TUBEFORGE_GEO_*`.
2. ~~tantivy + turso exact version pins~~ → **Resolved at gate:** turso `=0.7.2`, tantivy `=0.26.1`, tokio `1.53.1`, rusqlite `0.40.1` (dev), rustc 1.97.1.
3. ~~Thumbnail HTML→image method (SVG+resvg vs headless Chromium)~~ → **Resolved (Aug 4, 2026):** headless Chromium via **chromiumoxide 0.9.1** (CDP) + chromiumoxide_fetcher-pinned Chromium into `<data>/chromium` (rustls, no native-tls); Tailwind v4 compiled via standalone CLI (no Node); rationale — literal HTML+Tailwind v4, Blink determinism, pinned browser (no system Chrome dependency), permissive licensing.
4. Embedding strategy post-v1 (lexical-only in v1, ADR-9).
5. Windows CI target timing (post-macOS release).
6. ~~Undocumented YouTube API limits (`search.list` ~750-result cap; `playlistItems.list` 20k-video cap)~~ → **Resolved (Aug 4, 2026):** documented from MW Metadata wiki research — see §5.3 API behavior notes; RSS `feeds/videos.xml` has no playlist cap.
