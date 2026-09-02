**Product Requirements Document (PRD)**
**Project Name** — **TubeForge**
**Version** — 4.5 (Phases 0–6 delivered; Phase 7 Real-Time Creator Cockpit and Visual Growth Engine in progress)
**Date** — September 2, 2026
**Status** — Active Development — Dashboard Modernization Phase
**Intended License** — MIT or Apache-2.0 (all dependencies permissive — no BUSL, no copyleft)

> **v4.5 architecture update (Sep 2, 2026)** — Building upon the v4.0 native engine stack (`tfdb` WAL storage, from-scratch BM25, raw-Hyper, WebSocket JSON-RPC, SSE, Louvain Graph engine, and autonomous `greedy` daemon), Phase 7 introduces the **Unified Real-Time Creator Cockpit**. This upgrades TubeForge from fragmented analytical subpages into an integrated, evidence-grounded production workflow (Visual SERP Grid, Verifiable Evidence Ledger, In-Browser Kanban Board, Script Studio with Teleprompter, and Live 1280x720 Thumbnail Studio).

---

### 1. Overview
**TubeForge** is a local-first, open-source YouTube growth engine focused on helping creators build the perfect **Title, Description, Tags, and overall Video Strategy** through deep **SEO** and **GEO** optimization, combined with competitor analysis for organic growth.

**Data Sources (Flexible Free Policy)**
- Primary (always available, zero quota): Official YouTube Channel RSS feeds + Official oEmbed endpoint.
- Optional (rich metadata): Free YouTube Data API v3 — only when the user provides their own free API key in the `.env` file.
 - Secondary, opt-in: **yt-dlp** for transcripts (auto/manual captions), comments, video heatmap/live stats, and live SERP research (`TUBEFORGE_*`-gated) — local extraction, never page scraping.
 - **Local Whisper ASR fallback (shared with Vectron):** offline `vectron-whisper` crate (`whisper-rs` GGML, `symphonia`→`rubato` 16kHz mono) via path dep `../vectron/crates/vectron-whisper`. When `yt-dlp` captions miss, `transcript get --engine whisper|auto` extracts bestaudio via `yt-dlp` then transcribes locally with shared model cache `<data>/models/whisper/` — zero API cost, private. See PRD §5.2 + Vectron `AUDIO_ARCHITECTURE.md §1b`.
- **Filmot** (opt-in recovery, `TUBEFORGE_FILMOT_KEY`): raw JSON passthrough only, no DB writes.
- No page scraping of watch pages or channels.
- No paid services of any kind.

**Storage & Analytics**
- **TubeForge Database (`tfdb`)** — a from-scratch embedded storage engine written in pure Rust: single base path with two companion files (`<path>.wal` append-only checksummed write-ahead log + `<path>.dat` atomic checkpoint snapshot), zero-configuration, durable (fsync-on-commit), crash-safe (WAL replay + torn-tail truncation), single-writer. **Not** SQLite-format; portable by copying the snapshot. No rusqlite/SQLite escape hatch.
- **BM25 full-text scoring** via TubeForge's **own from-scratch engine** (`src/search`: inverted index + BM25 `k1=1.2, b=0.75`, atomic checksummed `index.json` snapshot).
- **Vector similarity** — HNSW ANN index ships (`src/tfdb/hnsw.rs`) but is **not yet wired** (no embeddings generated; deferred post-release).
- **Graph analytics** in Rust: PageRank centrality + Louvain community detection.
- All analytics computed locally in pure Rust; dashboard **delivered** via `tubeforge serve` (§5.4, loopback-only, raw-Hyper + WebSocket JSON-RPC + SSE).

**Core Design Principles**
- Free options only (no monetary charges ever).
- Optional free YouTube Data API for rich metadata (user-controlled via `.env`).
- Primary focus on **SEO + GEO** for perfect Titles, Descriptions, Tags, and Video Strategies.
- Support for bulk Channel (RSS) and Video Link ingestion.
- CLI-first with structured JSON output; dashboard delivered (`tubeforge serve`, loopback-only).
- Fully local and private.
- All configuration in `.env`.
- Pure Rust backend (tokio) + embedded `tfdb` engine + TubeForge-owned BM25.
- Single-writer, WAL-style journaling (crash-safe commit).
- Mandatory backup (snapshot copy + integrity re-open) before every batch ingest.
- Full cross-platform support (macOS first-class, then Linux, Windows).
- Thumbnail generation with HTML + latest Tailwind CSS and mandatory temporary `/assets` cleanup (Phase 3).
- Binary operable by humans and AI agents (Claude Code, Codex, OpenCode, Cursor, Hermes, Pi Agent, Harness CLI, etc.) via CLI + `--json` + **stdio JSON-RPC** (`tubeforge rpc`).
- Developed with Harness CLI / OpenCode and compatible agents.

**Primary Goal**
Enable creators to produce highly optimized Titles, Descriptions, Tags, and content strategies using free data (with optional rich API metadata), strong SEO/GEO signals, graph-powered analysis, and clear machine-readable output — while remaining completely free of monetary charges and open-source friendly.

### 2. Objectives
- Deliver the best possible free SEO and GEO recommendations for Titles, Descriptions, Tags, and Video Strategies.
- Support bulk ingestion of Channels (RSS) and individual Video Links.
- Optionally use the free YouTube Data API (user's own key) for rich metadata (tags, duration, full stats) via batched `videos.list` (1 unit per call, ≤50 IDs).
- Produce transparent 0–100 SEO/GEO scores with per-signal breakdowns, **enhanced by graph signals** (tag authority, topic dominance, keyword competition).
- Rank high-potential Next Ideas with transparent SEO/GEO scoring, **enhanced by graph-based gap detection**.
- Produce Competitor Scorecards, Health Reports, Keyword Rank Tracking, and Brand Alerts, **enhanced by graph centrality and community analysis**.
- Generate professional thumbnails with automatic `/assets` cleanup (Phase 3).
- **Content layer:** on-demand topic analysis (`analyze`), growth forecasting (OLS on `channel_snapshots`), packaging-psychology title formulas, and yt-dlp transcript extraction.
- Keep every secret only in `.env`.
- Store everything in a single portable, crash-safe local store (`tfdb`) with a tested snapshot/integrity backup.
- Ensure full cross-platform compatibility (macOS first, then Linux, Windows).
- Be operable by AI coding agents (CLI `--json`, exit codes, stdio JSON-RPC).
- Guarantee zero monetary charges at all times.
- **Integrate Knowledge Graph as internal enhancement to existing APIs — no separate `/api/kg/*` endpoints (YAGNI).**

### 3. Target Users
- YouTube creators focused on SEO/GEO-optimized content.
- Users who want optional rich metadata without any risk of being charged.
- Users who value a clean CLI with structured output.
- Developers and AI agents (Claude Code, Codex, OpenCode, Cursor, Harness, etc.).
- Users on macOS, Linux, and Windows.

### 4. Scope

**In Scope**
- Free data sources only: Official RSS + oEmbed (always) + optional free YouTube Data API (user-provided key) + opt-in local yt-dlp (transcripts/comments/research) + opt-in Filmot.
- Strong SEO + GEO focus for Title, Description, Tags, and Video Strategy.
- Bulk Video Link Ingestion and Channel management via text input.
- CLI-first: `init`, `ingest`, `refresh`, `score`, `ideas`, `keywords`, `tags`, `transcript`, `metadata`, `comments`, `gaps`, `outliers`, `scorecard`, `health`, `analyze`, `forecast`, `suggest`, `alerts`, `reindex`, `backup`, `quota`, `rpc`, `thumbnail`, `check`, `videos`, `export`, `filmot`, `prompt`, `serve`.
- Embedded `tfdb` store (custom WAL+snapshot format) + TubeForge-owned BM25 + Rust graph/vector analytics.
- Cross-platform (macOS first-class; Linux, Windows).
- Thumbnail Generator (Phase 3; HTML + Tailwind + mandatory cleanup).
- Agent operability: `--json` envelope, documented exit codes, **stdio JSON-RPC** via `tubeforge rpc`.
- All feature groups listed in Section 5.

**Out of Scope (v1 / post-release)**
- SQLite-format storage / rusqlite escape hatch (**replaced** by `tfdb` — v4.0 architecture decision, ADR-1).
- Any paid API or paid data source.
- Page scraping of YouTube watch pages or channels.
- Multi-tenant SaaS.
- Full automatic live A/B testing on YouTube.
- Browser extension or mobile apps.
- Wasm build (deferred).
- ANN vector wiring (HNSW ships but embeddings are not generated; deferred post-release).
- ML/GNN models (graph analytics via PageRank-class algorithms).
- General-purpose SQL server; multi-process concurrent writers.

### 5. Functional Requirements

#### 5.1 Configuration & Open-Source Friendliness
- All secrets live only in `.env` (including optional `YOUTUBE_API_KEY`).
- Complete `.env.example` provided.
- If no API key is present, the system runs fully on RSS + oEmbed (+ opt-in yt-dlp/Filmot).
- All third-party dependencies permissive (MIT/Apache-2.0) — no BUSL, no copyleft (`cargo-deny` gate).

#### 5.2 SEO + GEO Core (Highest Priority)
- Generate and score perfect Titles, Descriptions, and Tags.
- Full Video Strategy recommendations with GEO awareness.
- Deterministic SEO scoring engine (TubeForge-owned BM25 lexical signals via `src/search` + structural heuristics), 0–100 with per-signal breakdown.
- **18 SEO components** (see §15 for the full component list): 10 structural + 5 vidIQ/Phase-6.6 + **3 graph** (`tag_authority`, `topic_dominance`, `keyword_competition`).
- **7 GEO components** (free public signals only): `entity_coverage`, `qa_phrasing`, `list_phrasing`, `conversational`, `metadata_complete`, `location_signal` (recordingDetails), `topic_relevance` (topicDetails).
- **Packaging-psychology layer** (supporting, not blended into totals): five `TitleFormula` patterns — `TimeAnchor`, `PreciseNumber` (+extreme-outcome bonus), `IncomeClaim`, `ForbiddenKnowledge`, `HowToIdentity` — plus deterministic title variants.
- All scores persisted with component JSON for transparency and agent consumption.
- **Graph-aware scoring:** 3 additional components computed internally via Knowledge Graph, returned as `graph_scores` field on existing score endpoints. Defaults to `null` when KG not built (backward compatible).

#### 5.3 Data Ingestion
- **Channels**: Official RSS feeds (ETag-cached; ~15 most-recent entries; history requires API key).
- **Video Links**: Multi-line text box → extract IDs → fetch metadata.
  - With API key: full rich metadata via free `videos.list` (batched ≤50 IDs/call, 1 unit per call).
  - Without API key: official oEmbed only (title/author/thumbnail — documented limitation).
- Idempotent upsert semantics; rich source wins on conflict (api > oembed > rss).
- Backup guard (`tfdb` snapshot + integrity re-open) runs before every batch ingest.
- All data stored in the single embedded `tfdb` store and linked for analysis.

#### 5.4 Dashboard (v4.5 Svelte 5 Architecture)
- Dashboard **delivered** as `tubeforge serve [--port] [--host]`; served by a **raw-Hyper web framework** with static asset serving.
- **Frontend Architecture**: **Svelte 5 (Runes `$state`, `$derived`, `$effect`) + Vite + Tailwind CSS v4**. Compiler-first architecture eliminating Virtual DOM overhead for surgical, high-throughput real-time DOM updates.
- **Single-Binary Embedding**: Compiles to an ultralight bundle (~35–50 KB gzipped) embedded directly into the Rust binary, ensuring zero-dependency, local-first execution.
- **Loopback-only** binding (127.0.0.1 default; `localhost`/`::1` allowed; any non-loopback `--host` → rejected, exit 2). Port precedence: flag > `TUBEFORGE_SERVE_PORT` > 8080. Single-user, no auth.
- **WebSocket JSON-RPC** at `/ws`: `{id, method, params}` in; tagged `progress` / `result` / `error` / `notification` out. Methods include `dashboard.overview`, `ideas.analyze`, `keywords.*`, `scores.*`, `videos.*`, `scorecard.get`, `health.get`, `gaps.*`, `tags.*`, `analysis.*`, `alerts.*`, `audit.get`, `channels.snapshots`, `kanban.*`. Progress streams precede final `result`.
- **SSE** at `/events`: streams `counts` and real-time greedy daemon / ingestion events every 5s (only on change) with a 15s `: ping` heartbeat.
- **HTTP API** under `/api/`: REST endpoints for health, counts, trends, scores, videos, kanban, gaps, tags, transcripts, and analysis.
- **Creator Workflows**: Unified 5-stage Cockpit (Research ➔ Evidence Ledger ➔ Ideas ➔ Script Studio with 60fps Teleprompter ➔ Live Thumbnail Studio ➔ Kanban Board).
- **CSRF policy**: POSTs guarded by Origin/Referer check — presented origin's host:port must match the bound address (mismatch → 403); absent headers allowed (curl/scripts/agents can't be browser-CSRF'd).
- **Concurrency caveat (single-writer)**: `serve` opens one shared Db and mutates only via CLI code paths; do NOT run `serve` concurrently with writing CLI commands (snapshot/WAL readers fine).

#### 5.5 Grow Audience
- Next Ideas, Saved Ideas (draft/saved/discarded), Niches.
- A/B Test Manager (manual, post-v1).
- Retention Analyzer (Studio CSV import, post-v1).
- Keyword Rank Tracking (snapshots + trends).
- **Graph-based idea generation:** Ideas enhanced by community gap detection (low-centrality communities indicate opportunity).

#### 5.6 Performance
- Health Report, Competitor Scorecard (incl. PageRank centrality), Brand Alerts.
- **Graph-aware scorecard:** Channel centrality rankings via PageRank, community membership via Louvain algorithm.
- **Growth forecasting** (`forecast`): weighted OLS on elapsed time with recency half-life (30d), `MIN_POINTS=3`, t-stat ±2.0 significance gate, `TREND_THRESHOLD_PCT=10%` → Rising/Flat/Falling, LOW/MEDIUM/HIGH reliability; `next_estimate`, `slope_per_day`, `r_squared`. Fed by `channel_snapshots`.

#### 5.7 Operations
- Channel Settings, Channel Backup, Canned Responses, Promo Materials (post-v1).
- **Thumbnail Generator** (Phase 3):
  - Raw assets in temporary `/assets`.
  - HTML + latest Tailwind CSS.
  - Immediate automatic deletion of raw assets after successful generation (`--keep-assets` debug-only; RAII Drop guard + error-path cleanup).
- **Backup (v1, mandatory):** `tfdb` snapshot copy + integrity re-open + retention, auto-run before every batch ingest.

#### 5.8 Competitor Input
- Free-form text (Channel ID, @handle, or URL) + Video Link Ingestion.

#### 5.9 Agent & Binary Operability
- Native Binary (macOS / Linux / Windows).
- Clean CLI + `--json` structured output + documented exit codes (0/1/2/3/4/5).
- **Stdio JSON-RPC bridge** via `tubeforge rpc` for Claude Code, Codex, OpenCode, Cursor, Hermes, Pi Agent, Harness CLI, and similar agents (same method surface as the WebSocket dashboard, over line-delimited stdin/stdout).

#### 5.10 Data Source Policy
- Always available: Official RSS + oEmbed (zero quota).
- Optional: Free YouTube Data API (user's own key in `.env`).
  - Used only when present.
  - Heavily batched and cached (ETag; per-day quota ledger).
  - Quota usage visible via `tubeforge quota`.
  - No monetary charges possible.
- Opt-in local: yt-dlp (transcripts, comments, heatmap, SERP research) + Filmot (recovery passthrough).
- No scraping.

### 6. Technical Architecture
*(Full detail in `HLD.md` and `LLD.md`)*

**Language & Development**
Pure Rust • Harness CLI / OpenCode • Tokio only.

**Backend**
Single binary CLI (clap + tokio). One optional long-running process: `tubeforge serve` (loopback-only dashboard, §5.4); everything else daemon-free, no ports.

**Storage — `tfdb` (from-scratch embedded engine, pure Rust)**
- Two companion files per base path: `<path>.wal` (append-only, checksummed WAL) + `<path>.dat` (atomic checkpoint snapshot). Magics `TFWL`/`TFDT`; rows serialized via deterministic serde_json.
- Durable & crash-safe: `begin`→`Tx` stages in memory; `commit` writes one WAL record + `fsync` (default on) then applies to the live snapshot; crash replays WAL and truncates torn tail; `checkpoint()` writes full snapshot via temp-file + `rename` (atomic) then truncates WAL. Committed data never lost; partial transactions never appear.
- Typed-row model (`TableSchema`/`Col`), 22 tables (see §15), no SQL DDL; queries via Rust scans (`src/tfdb/query.rs`: `sum/avg/min/max/group_counts/join`). `SCHEMA_VERSION = 9` in the `meta` table.
- Single-writer by design (one write lock serializes writers; reads from in-memory snapshot).
- **Not SQLite-compatible; no rusqlite escape hatch** (ADR-1).

**Analytics — Rust-owned**
- BM25: TubeForge's own from-scratch engine (`src/search`: inverted index + `k1=1.2, b=0.75`, atomic checksummed `index.json` snapshot at `<data>/index/`). Not dependent on any external index engine.
- Vector: HNSW ANN ships (`src/tfdb/hnsw.rs`) but unwired (no embeddings generated; deferred).
- Graph: adjacency + PageRank + Louvain in Rust.

**Thumbnail Pipeline (Phase 3)**
HTML + latest Tailwind CSS → render (headless Chromium via chromiumoxide 0.9.1, pinned fetcher Chromium into `<data>/chromium/`) → immediate `/assets` cleanup.

**Binary / Wasm + Cross-Platform**
Native binaries for macOS (first), Linux, Windows. Wasm deferred.

**Data + Analytics Pipeline**
1. Load `.env` (detect optional API key).
2. Backup guard (snapshot copy + integrity re-open).
3. Resolve competitors and video links.
4. Fetch RSS + (oEmbed or rich API metadata); ETag caching; quota ledger.
5. Upsert into `tfdb` (idempotent, single transaction).
6. Update BM25 index.
7. Compute SEO/GEO scores, ideas, scorecards, alerts, and analytics datasets.
8. Output to CLI tables and/or `--json` for agents; stdio JSON-RPC bridge (`tubeforge rpc`) for agent harnesses.

### 7. Non-Functional Requirements
- Zero monetary charges at all times.
- Optional rich metadata via free YouTube Data API (user-controlled).
- SEO/GEO first.
- CLI-first with stable JSON contracts; dashboard delivered (`serve` — the one long-running, non-JSON-envelope command).
- Single-file-portable crash-safe storage with snapshot backup + integrity re-open.
- Cross-platform (macOS first; Linux, Windows).
- Agent-operable (`--json`, exit codes, stdio JSON-RPC).
- High performance (interactive CLI at 1–10k videos; no ANN wiring needed at this scale).
- Reliability: mandatory backups, fsync-on-commit WAL, integrity checks, version pinning.
- Privacy, security (secrets only in `.env`), open-source readiness.

### 8. Success Metrics
- Users can generate high-quality SEO/GEO-optimized Titles, Descriptions, Tags, and Strategies (verified via transparent score breakdowns).
- Users can optionally obtain rich metadata (tags, duration, full stats) without any charges.
- `tubeforge score` returns deterministic, explainable scores with component JSON.
- System works fully with no API key (RSS + oEmbed only; documented data limits).
- Ingest is idempotent: running twice yields identical state.
- Backup round-trip passes integrity re-open in CI (reopen snapshot with `Db::open`).
- Thumbnail Generator always cleans `/assets` (Phase 3).
- Binary works on macOS first; Linux/Windows targets configured.
- CLI is agent-operable: documented exit codes + JSON envelope; stdio JSON-RPC bridge functional.
- `analyze`/`forecast` return deterministic, explainable content and growth outputs.

### 9. Implementation Phases
*(Component-level detail in `HLD.md` §12 and `LLD.md` §13)*

**Phase 0 – Skeleton + Engine Gate — ✅ COMPLETE (Aug 3, 2026)**
Repo, `.env.example`, engine + tantivy pinned, **M4 smoke gate PASSED 5/5** (CRUD/WAL/backup round-trip/integrity_check/rusqlite-open; FTS probe confirmed engine FTS unavailable → tantivy-direct validated), CLI skeleton (`init`/`ingest links`/`backup`/`quota`), error taxonomy + exit codes. *(Storage engine later replaced by `tfdb` — see Phase 6.)*

**Phase 1 – Foundation — ✅ COMPLETE (Aug 3, 2026)**
Fetch (RSS/oEmbed/API+quota ledger), full v1 schema + migration runner, Ingest (idempotent upsert, source precedence, backup guard, ingest_log), tantivy index + `reindex`, CLI: `ingest channels`/`ingest links`/`refresh`/`score`(basic)/`backup`/`quota`/`rpc`, 35/35 tests green, clippy clean, live RSS smoke verified (15 videos, Google for Developers). *(Index later replaced by TubeForge-owned BM25 — Phase 6.)*

**Phase 2 – SEO/GEO Intelligence + Analytics — ✅ COMPLETE (Aug 4, 2026)**
Full scoring engine (10 SEO + 7 GEO components, env-configurable weights, defaults sum 1.0; new free signals `location_signal` from `recordingDetails`, `topic_relevance` from `topicDetails.topicCategories`), analytics (PageRank graph, Next Ideas, keyword rank tracking, scorecard/health/alerts), CLI `ideas`/`keywords`/`scorecard`/`health`/`alerts` + `--json` envelopes, ingest hardening (bare-ID checksums, extended URL-form parsing, 32-category map, disabled-metric heuristic), migration 002 (SCHEMA_VERSION 1→2), 89/89 tests green, clippy clean.

**Phase 3 – Thumbnails & Polish — ✅ COMPLETE (Aug 4, 2026)**
Thumbnail Generator (`tubeforge thumbnail render|list-templates`; HTML + Tailwind CSS v4 templates, headless Chromium via **chromiumoxide 0.9.1** + pinned fetcher Chromium, 1280×720 PNG; **mandatory `/assets` cleanup** via RAII Drop guard + error-path cleanup, `--keep-assets` debug-only), `check availability` (batched `videos.list` part=snippet,status; missing IDs → `video_unavailable` alerts; `privacy_status` column via migration 003, SCHEMA_VERSION 2→3, health `privacy` census), `export` (`--format zip|dir`; manifest.json + CSVs + JSON arrays; zip crate 8.6, deterministic), `filmot get` (opt-in key, raw JSON passthrough, no DB writes), agent interface hardening (stdout/stderr separation, `list_alerts(0)` LIMIT fix, nested `check availability`, `all_ideas()`, `tests/agent_contract.rs` 11 binary-level `--json` contract tests), **135/135 tests + 1 ignored (Chromium-gated render), clippy clean**.

**Phase 4 – Hardening & Release — IN PROGRESS (Aug 4, 2026)**
Done: performance smoke gate **PASSED on M4** (5k videos: ingest 20.2s + reindex 0.2s + ideas 0.06s = 20.5s vs 30s budget, release profile); release prep (LICENSE-MIT/LICENSE-APACHE, Cargo.toml metadata incl. rust-version 1.85 + license field, CHANGELOG 0.1.0, GitHub Actions CI matrix macOS-14/ubuntu/windows: build+clippy+test, README command table + cross-platform notes); cargo audit clean; cargo deny clean; **dashboard delivered** (v3.14 `serve` — loopback-only, vendored htmx 2.0.9, CSRF-guarded POSTs, inline SVG charts). **Remaining:** user actions (create GitHub repo, set remote, push, tag v0.1.0) + fmt pass (deferred, optional).

**Phase 6 – Engine Independence + Web RPC — ✅ COMPLETE (Aug 14, 2026, after v3.15)**
Removed external engine/index/web dependencies in favor of TubeForge-owned components: **`tfdb` storage engine** replaces Turso/SQLite (`80666fb`); **from-scratch BM25** replaces tantivy (`02ae3ae`); **raw-Hyper web framework** replaces Axum (`fee8e3d`); **SSE replaces htmx polling** (`489a289`); **WebSocket JSON-RPC** surface (`/ws`, ~21 methods) + content/`analyze` layer (`analyze`, `transcript`, `growth`/`forecast`, `suggest`, `tags`, `gaps`, `videos dedupe`, `metadata`, `comments`), packaging-psychology scoring, HNSW module (unwired), KG fully integrated (kg_entities/kg_relations/kg_communities, PageRank + Louvain, `graph_scores`). SCHEMA_VERSION = 9. Tests + clippy green.

**Phase 7 – Unified Real-Time Creator Cockpit & Visual Growth Engine — 🚀 IN PROGRESS (Sep 2, 2026)**
Refines the TubeForge dashboard from isolated subpages into an integrated, evidence-grounded production workspace:
1. **Visual SERP Grid & Media Cards**: 16:9 thumbnail previews, channel badges, and visual outlier performance multipliers (e.g. `8.21x Breakout`).
2. **Verifiable Evidence Ledger**: Direct citation cards linking 18 SEO + 7 GEO algorithmic scores to exact BM25 documents, competitor videos, and tag clusters.
3. **Interactive Production Kanban Board**: Full in-browser drag-and-drop workflow tracking (`todo` ➔ `inprogress` ➔ `done` ➔ `published`) directly connected to 0:00–0:45 First-Screen retention prompt contracts.
4. **Script Studio & Recording Teleprompter**: WPM-controlled script prompter with spacebar playback, cue markers, and timer HUD.
5. **Live 1280x720 Thumbnail Preview Studio**: Real-time visual template editing for high-contrast, zero-face developer thumbnails.
6. **Real-Time WebSocket & SSE Event Ticker**: Live streaming feed of autonomous topic hunting (`greedy daemon`), keyword rank position deltas, and database transactions.

**Phase 4 (cont.) – Release**
Performance, documentation, cross-platform testing (Linux, Windows), public open-source release of **TubeForge** (MIT/Apache-2.0).

### 10. Assumptions & Risks
**Assumptions**
Official RSS, oEmbed, and free YouTube Data API remain available • User understands the free daily quota (no monetary charge) • yt-dlp remains installable/usable locally for opt-in transcript/research features • RSS remains an undocumented best-effort endpoint (~15 recent entries) • `tfdb` engine (own code) remains under our control for durability fixes.

**Risks & Mitigations**
- **Storage durability of a from-scratch engine** (own code, no upstream maintainer) → fsync-on-commit WAL; atomic checkpoint via temp-file + rename; WAL replay + torn-tail truncation on open; snapshot backup before every batch ingest; integrity re-open after backup; SCHEMA_VERSION pinning.
- **No SQLite escape hatch** (ADR-1) → snapshot `.dat` is fully self-contained and portable; `backup` produces a standalone tfdb checkpoint (`tubeforge-<ts>.db`) verifiable by re-open. Migration tooling to a future engine is a post-release concern.
- **API quota exhaustion** → `tubeforge quota` warning + automatic fallback to RSS + oEmbed.
- **Limited oEmbed data when no key is present** → Document clearly.
- **Single-writer constraint** (`tfdb` one write lock) → sequential pipeline, single writer by design; snapshot readers safe; don't run `serve` alongside writing CLI commands.
- **HNSW unwired** → vector retrieval not exposed; BM25 lexical retrieval is the shipped path; defer embedding pipeline.
- **yt-dlp external-process dependency** → gated behind opt-in flags; degrades gracefully; never page-scrapes.
- **Component-count inconsistency** (15 non-graph SEO keys surfaced via API/RPC vs 18 total incl. graph components) → graph components flow through `graph_scores`; runtime fresh scores use graph=null → 0; documented in §15.
- Cross-platform differences → Early testing; macOS first.
- Scope creep → Strict phase gating; ANN wiring / Wasm / GNN deferred.

### 11. Open Questions
- ~~Exact on-disk format~~ → **Resolved:** `tfdb` custom `.wal` + `.dat`, not SQLite (ADR-1, v4.0).
- ~~Preferred lightweight charting approach~~ → **Resolved (Aug 4, 2026):** server-rendered inline SVG in Rust — no JS chart libraries.
- ~~Preferred HTML-to-image method for thumbnails (SVG+resvg vs headless Chromium)~~ → **Resolved (Aug 4, 2026):** headless Chromium via **chromiumoxide 0.9.1** — literal HTML+Tailwind v4 rendering, Blink determinism, pinned fetcher Chromium (no system Chrome dependency), MIT/Apache-2.0, actively maintained.
- ~~Degree of Wasm support in v1~~ → **Resolved:** deferred post-v1.
- ~~Default local UI authentication~~ → **Not applicable:** CLI-only v1.
- ~~SEO/GEO scoring spec~~ (signal weights/formulas — §5.2) → **Resolved (Aug 4, 2026):** documented defaults baked in (10 SEO + 7 GEO components, each set sums 1.0) with per-component env overrides; extended (Phase 6.6) to 18 SEO components; packaging-psychology as supporting layer.
- ~~HTMX dashboard delivery~~ → **Resolved:** delivered, then **re-architected (Aug 14, 2026):** SSE replaces htmx polling; raw-Hyper replaces Axum; WebSocket JSON-RPC added.
- ~~Vector indexing approach~~ → **Resolved:** HNSW module ships (`src/tfdb/hnsw.rs`) but is **unwired** (no embeddings); deferred post-release.

---

### 12. CLI Command Reference (v4.0)

Global flags: `--json`, `--verbose`, `--db-path`, `--config`.

| Command | Sub-commands / purpose |
|---|---|
| `init` | Create data dir, `.env`, schema |
| `ingest` | `channels` (RSS), `links` (oEmbed/API) — idempotent, backup-guarded |
| `refresh` | Re-fetch channel(s); `--channel`, `--no-backup` |
| `score` | Score stored video or draft; `--video-id`, `--draft-title/-desc/-tags`, `--keywords`; graph-aware |
| `ideas` | Next ideas; `--limit`, `--niche`, `--status` |
| `keywords` | `add`, `check`, `report`, `inspect`, `research`, `discover` |
| `tags` | `backfill`, `analyze` |
| `transcript` | `get`, `list`, `clear` (yt-dlp captions → `transcripts` table; `--engine auto|ytdlp|whisper` — `whisper` via shared `vectron-whisper` Rust crate, local GGML fallback when captions disabled) |
| `metadata` | Video heatmap / live stats via yt-dlp |
| `comments` | `get`, `list`, `clear` |
| `gaps` | Content/tag gap analysis; `--channel`, `--markdown`; `outliers` |
| `scorecard` | Competitor scorecard (PageRank centrality, Louvain communities) |
| `health` | Health report + privacy census |
| `analyze <topic>` | Realtime yt-dlp SERP research → demand-supply gap + auto-drafted packaging |
| `forecast` | Growth forecast from `channel_snapshots`; `--horizon`, `--channels` |
| `suggest <topic>` | Next-video recommendations with view prediction + "why" |
| `alerts` | `list`, `clear`, `--mark-read` |
| `reindex` | Rebuild BM25 index from stored videos |
| `backup` | Snapshot copy + integrity re-open + retention |
| `quota` | Per-day API quota ledger |
| `rpc` | Stdio JSON-RPC bridge for agent harnesses (OpenCode, Claude Code, Codex, Hermes, Pi Agent) — long-running; stdout reserved for responses |
| `thumbnail` | `render`, `list-templates` |
| `check` | `availability` (batched `videos.list` part=snippet,status) |
| `videos` | `dedupe` |
| `export` | `--format zip\|dir` (manifest.json + CSVs + JSON arrays) |
| `filmot` | `get` (opt-in recovery passthrough) |
| `prompt` | Print agent/usage prompt |
| `serve` | Dashboard (loopback-only; the one long-running, non-JSON command) |

**Exit codes** (`src/error.rs`): `0` success • `1` runtime/storage/config/index • `2` usage (clap) • `3` fetch/parse • `4` quota exhausted • `5` integrity failure.

**`--json` envelope** (`src/output.rs`): `{ "ok": bool, "data": ?, "meta": ?{duration_ms, quota?}, "error": ?{code, message, source?, item?} }`. Error codes: `CONFIG`, `FETCH`, `PARSE`, `QUOTA_EXHAUSTED`, `STORAGE`, `INTEGRITY`, `INDEX`, `USAGE`, `NOT_IMPLEMENTED`.

---

### 13. JSON-RPC Surface (WebSocket `/ws` + stdio `tubeforge rpc`)

One JSON-RPC protocol, two transports:
- **WebSocket** at `/ws` — the dashboard/frontend talks to the running `tubeforge serve`.
- **stdio** (`tubeforge rpc`) — agent harnesses (OpenCode, Claude Code, Codex, Hermes, Pi Agent) spawn the binary and speak the same line-delimited JSON-RPC on stdin/stdout for analysis; the tfdb database is the storage source, the frontend provides visual analysis.

Envelope in: `{"id", "method", "params"}`. Envelope out: tagged `progress {id, progress, message}` → `result {id, data}` | `error {id, error:{code, message}}` | `notification {event, data}`. Parse errors return `-32700`; unknown methods / internal errors return `-32603`. stdout on the stdio transport carries **only** responses (one JSON per line, flushed per message).

Methods: `dashboard.overview`, `ideas.analyze`, `keywords.list`, `keywords.trending`, `scores.list`, `scores.detail`, `scores.backfill`, `videos.list`, `videos.detail`, `scorecard.get`, `health.get`, `gaps.get`, `tags.cloud`, `tags.gaps`, `analysis.overview`, `analysis.next-video`, `analysis.keywords`, `analysis.refresh`, `alerts.list`, `audit.get`, `channels.snapshots`, `kanban.list`, `kanban.create`, `kanban.move`, `kanban.prompt`.

---

### 14. Knowledge Graph Integration (v3.0 — internal-only)

Architecture decision: **KG is an internal-only enhancement to existing APIs. NO `/api/kg/*` endpoints (YAGNI).** All KG processing happens inside existing handlers; the frontend consumes KG signals as optional fields on existing endpoints (`graph_scores` on scores, `centrality` on scorecard, graph ideas/gaps on ideas/gaps).

- **Entities (6):** Video, Channel, Tag, Keyword, Topic, Entity.
- **Relations (9):** `tags`, `created_by`, `about_topic`, `competes_in`, `dominates`, `related_to`, `similar_to`, `mentioned_in`, `contains`. Weighted (e.g. tag weight `1/(1+pos)`, keyword weight `1/(1+position)`).
- **Algorithms:** Louvain community detection + PageRank centrality (`kg_algorithms.rs`).
- **Build:** `BuildMode::Full` (clears KG tables) or `Incremental` (`kg_builder::build`).
- **Load:** `kg_builder::load_or_build()` — checks `meta.kg_cache_json`; on miss full-rebuilds then `load_from_db`; lazy-loaded in `serve.rs` via double-checked locking (`AppState.kg`), cached for server lifetime; graceful degradation to `null` when KG empty/not built (backward compatible).
- **Persistence:** `kg_entities` (entity_id, entity_type, canonical_name, display_name, properties, embedding [unused], centrality, community_id, source, source_ref), `kg_relations` (from, to, relation_type, weight, source), `kg_communities` (community_id, community_type, summary, member_count, mean_views, mean_seo_score, top_entities).
- **Consumers:** `graph_aware::compute_graph_scores` (tag_authority/topic_dominance/keyword_competition), `generate_graph_ideas`, `find_content_gaps`, `compute_tag_authority_by_name`.

---

### 15. Storage Schema + Scoring Components

**`tfdb` tables (22):** `meta`, `channels`, `videos`, `competitors`, `keywords`, `keyword_rankings`, `scores`, `ideas`, `edges`, `alerts`, `ingest_log`, `tags`, `video_tags`, `competitor_tags`, `transcripts`, `comments`, `video_heatmap`, `channel_snapshots`, `keyword_research`, `kg_entities`, `kg_relations`, `kg_communities`. `SCHEMA_VERSION = 9`.

**SEO components (18):**
- Structural (10): `keyword_title`, `title_front`, `title_length`, `title_hooks`, `keyword_desc`, `desc_first150`, `desc_structure`, `tags_relevance`, `tags_quality`, `keyword_tags`
- vidIQ/Phase-6.6 (5): `title_40_chars`, `desc_first2lines`, `desc_length`, `hashtag_count`, `keyword_triple`
- Graph (3, via `graph_scores`): `tag_authority`, `topic_dominance`, `keyword_competition`

**GEO components (7):** `entity_coverage`, `qa_phrasing`, `list_phrasing`, `conversational`, `metadata_complete`, `location_signal`, `topic_relevance`.

**Packaging-psychology (supporting):** `TitleFormula` — `TimeAnchor`, `PreciseNumber`, `IncomeClaim`, `ForbiddenKnowledge`, `HowToIdentity` (+ extreme-outcome bonus); deterministic `variants()`.

---

**Project Name for Open-Sourcing:** **TubeForge**

This is the complete Product Requirements Document (Version 4.5).

---

### 16. Changelog

**v3.10 update (Aug 3, 2026):** Implementation status — **Phase 0 ✅ and Phase 1 ✅ complete** (engine gate 5/5, 35/35 tests, clippy clean, live RSS smoke verified). Next: **Phase 2 — SEO/GEO Intelligence + Analytics.**

**v3.11 update (Aug 4, 2026):** Implementation status — **Phase 2 ✅ complete** (full SEO/GEO scoring engine with 2 new GEO signals, analytics suite, 5 new CLI commands, ingest hardening from MW Metadata research — ID checksums, category map, disabled-metric heuristic — 89/89 tests, clippy clean). Next: **Phase 3 — Thumbnails & Polish.**

**v3.12 update (Aug 4, 2026):** Implementation status — **Phase 3 ✅ complete** (thumbnail generator via chromiumoxide + pinned Chromium with mandatory `/assets` cleanup, `check availability` + `privacy_status` migration 003, `export` with CSVs/ZIP, Filmot opt-in recovery, agent interface hardening — 135/135 tests + 1 ignored Chromium-gated, clippy clean). Next: **Phase 4 — Hardening & Release.**

**v3.13 update (Aug 4, 2026):** Implementation status — **Phase 4 — IN PROGRESS** — performance smoke gate **PASSED on M4** (5k videos: ingest 20.2s + reindex 0.2s + ideas 0.06s = 20.5s vs 30s budget, release profile); release prep complete (LICENSE-MIT/LICENSE-APACHE, Cargo.toml metadata, CHANGELOG 0.1.0, CI matrix macOS-14/ubuntu/windows, README command table + cross-platform notes); cargo audit + cargo deny clean; 136/136 tests + 2 ignored (perf gate + Chromium render), clippy clean. Remaining: user actions (repo creation, push, tag v0.1.0) + optional fmt pass.

**v3.14 update (Aug 4, 2026):** Implementation status — **Phase 4 — IN PROGRESS** — the deferred §5.4 dashboard is **✅ DELIVERED**: `tubeforge serve [--port] [--host]` (loopback-only; non-loopback host → exit 2; `TUBEFORGE_SERVE_PORT` env), Axum 0.8.9 + askama 0.14 (compile-time autoescaping) + vendored htmx 2.0.9 (`static/htmx.min.js`, offline), pages `/` `/scores` `/ideas` `/keywords` `/alerts` `/scorecard` `/health` `/healthz`, CSRF origin guard on POSTs (mismatch → 403; absent headers allowed for curl/agents), server-rendered inline SVG charts (no JS libs), single-writer Db caveat (don't run alongside writing CLI commands; WAL readers fine), `serve` emits no JSON envelope (stdout stays empty). 165/165 tests + 2 ignored (perf gate + Chromium render), clippy clean, cargo-deny green.

**v3.15 update (Aug 8, 2026):** Architecture decision — **Knowledge Graph is internal-only enhancement** (see `PRD-KNOWLEDGE-GRAPH.md` v3.0). **NO** separate `/api/kg/*` endpoints. KG signals returned as optional fields on existing endpoints (`graph_scores` on scores, `centrality` on scorecard, etc.). Lazy-loaded via `kg_builder::load_or_build()`. Graceful degradation when KG not built. Full detail in `FRONTEND-BACKEND-MAP.md` and `PRD-KNOWLEDGE-GRAPH.md`.

**v4.0 update (Aug 14, 2026):** **Stack re-architecture** — removed external engine/index/web dependencies; all heavy lifting now TubeForge-owned (`tfdb` engine, from-scratch BM25, raw-Hyper, WebSocket JSON-RPC, SSE, content layer, KG integrated).

**v4.5 update (Sep 2, 2026):** **Phase 7 — Unified Real-Time Creator Cockpit & Visual Growth Engine (IN PROGRESS)**:
- Upgrades TubeForge dashboard to a continuous creator workflow.
- Visual media cards with 16:9 thumbnail previews and outlier badges.
- Verifiable Evidence Ledger linking algorithmic scores to source documents and videos.
- In-browser interactive video production Kanban board with drag-and-drop lifecycle transitions and retention prompt contracts.
- In-browser recording teleprompter with pace/cue HUD.
- Live Chromium thumbnail preview canvas.
- Real-time WebSocket event ticker for autonomous greedy daemon and SERP position deltas.

---

*Companion documents: `ROADMAP.md` (v1.0), `HLD.md` (v1.3), `LLD.md` (v1.6), `PRD-KNOWLEDGE-GRAPH.md` (v3.0), `FRONTEND-BACKEND-MAP.md`.*
