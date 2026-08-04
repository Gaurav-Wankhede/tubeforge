**Product Requirements Document (PRD)**  
**Project Name:** **TubeForge**  
**Version:** 3.13 (Phases 0–3 Complete)  
**Date:** August 4, 2026  
**Status:** Ready for Implementation (Phase 4)  
**Intended License:** MIT or Apache-2.0 (all dependencies permissive: Turso MIT, tantivy MIT)

### 1. Overview
**TubeForge** is a local-first, open-source YouTube growth engine focused on helping creators build the perfect **Title, Description, Tags, and overall Video Strategy** through deep **SEO** and **GEO** optimization, combined with competitor analysis for organic growth.

**Data Sources (Flexible Free Policy)**
- Primary (always available, zero quota): Official YouTube Channel RSS feeds + Official oEmbed endpoint.
- Optional (rich metadata): Free YouTube Data API v3 — only when the user provides their own free API key in the `.env` file.
- No page scraping of any kind.
- No paid services of any kind.

**Storage & Analytics**
- **Turso Database** (embedded, MIT) — a from-scratch SQLite re-implementation in Rust: single-file `.db`, SQLite file-format compatible, zero-configuration, durable, crash-safe in normal operation, portable back to SQLite at any time.
- **BM25 full-text scoring** via the **tantivy** crate (owned by TubeForge's Rust code, not the engine's experimental index modules).
- **Vector similarity** (brute-force cosine in Rust) and **graph analytics** (PageRank in Rust).
- All analytics computed locally in pure Rust; the HTMX Dashboard is deferred (CLI-only v1).

**Core Design Principles**
- Free options only (no monetary charges ever).
- Optional free YouTube Data API for rich metadata (user-controlled via `.env`).
- Primary focus on **SEO + GEO** for perfect Titles, Descriptions, Tags, and Video Strategies.
- Support for bulk Channel (RSS) and Video Link ingestion.
- CLI-first with structured JSON output; HTMX dashboard deferred to post-v1.
- Fully local and private.
- All configuration in `.env`.
- Pure Rust backend (tokio) + embedded Turso Database + tantivy.
- WAL journal mode only (never MVCC — engine limitation verified in issue tracker).
- Mandatory backup (VACUUM INTO + integrity_check) before every batch ingest.
- Full cross-platform support (macOS first-class, then Linux, Windows).
- Thumbnail generation with HTML + latest Tailwind CSS and mandatory temporary `/assets` cleanup (Phase 3).
- Binary operable by humans and AI agents (Claude Code, Codex, OpenCode, Cursor, Harness CLI, etc.) via CLI + JSON + MCP server.
- Developed with Harness CLI / OpenCode and compatible agents.

**Primary Goal**  
Enable creators to produce highly optimized Titles, Descriptions, Tags, and content strategies using free data (with optional rich API metadata), strong SEO/GEO signals, graph-powered analysis, and clear machine-readable output — while remaining completely free of monetary charges and open-source friendly.

### 2. Objectives
- Deliver the best possible free SEO and GEO recommendations for Titles, Descriptions, Tags, and Video Strategies.
- Support bulk ingestion of Channels (RSS) and individual Video Links.
- Optionally use the free YouTube Data API (user's own key) for rich metadata (tags, duration, full stats) via batched `videos.list` (1 unit per call, ≤50 IDs).
- Produce transparent 0–100 SEO/GEO scores with per-signal breakdowns.
- Rank high-potential Next Ideas with transparent SEO/GEO scoring.
- Produce Competitor Scorecards, Health Reports, Keyword Rank Tracking, and Brand Alerts.
- Generate professional thumbnails with automatic `/assets` cleanup (Phase 3).
- Keep every secret only in `.env`.
- Store everything in a single SQLite-format file with a tested escape hatch to SQLite.
- Ensure full cross-platform compatibility (macOS first, then Linux, Windows).
- Be operable by AI coding agents (CLI `--json`, exit codes, MCP).
- Guarantee zero monetary charges at all times.

### 3. Target Users
- YouTube creators focused on SEO/GEO-optimized content.
- Users who want optional rich metadata without any risk of being charged.
- Users who value a clean CLI with structured output.
- Developers and AI agents (Claude Code, Codex, OpenCode, Cursor, Harness, etc.).
- Users on macOS, Linux, and Windows.

### 4. Scope

**In Scope**
- Free data sources only: Official RSS + oEmbed (always) + optional free YouTube Data API (user-provided key).
- Strong SEO + GEO focus for Title, Description, Tags, and Video Strategy.
- Bulk Video Link Ingestion and Channel management via text input.
- CLI-first: `init`, `ingest`, `score`, `ideas`, `keywords`, `scorecard`, `health`, `alerts`, `backup`, `quota`, `reindex`.
- Embedded Turso Database (SQLite-compatible single file) + tantivy BM25 + Rust vector/graph analytics.
- Cross-platform (macOS first-class; Linux, Windows).
- Thumbnail Generator (Phase 3; HTML + Tailwind + mandatory cleanup).
- Agent operability: `--json` envelope, documented exit codes, MCP via `tursodb --mcp`.
- All feature groups listed in Section 5.

**Out of Scope (v1)**
- HTMX dashboard (deferred post-v1).
- Any paid API or paid data source.
- Page scraping of YouTube watch pages or channels.
- Multi-tenant SaaS.
- Full automatic live A/B testing on YouTube.
- Browser extension or mobile apps.
- Wasm build (deferred; Turso supports it, FTS needs opt-in flag — revisit post-v1).
- ANN vector indexing (not available upstream; unnecessary at v1 scale).
- ML/GNN models (graph analytics via PageRank-class algorithms).
- General-purpose SQL server; MVCC journal mode.
- Permanent storage of raw thumbnail assets.

### 5. Functional Requirements

#### 5.1 Configuration & Open-Source Friendliness
- All secrets live only in `.env` (including optional `YOUTUBE_API_KEY`).
- Complete `.env.example` provided.
- If no API key is present, the system runs fully on RSS + oEmbed.
- All third-party dependencies permissive (MIT/Apache-2.0) — no BUSL, no copyleft.

#### 5.2 SEO + GEO Core (Highest Priority)
- Generate and score perfect Titles, Descriptions, and Tags.
- Full Video Strategy recommendations with GEO awareness.
- Deterministic SEO scoring engine (BM25 lexical signals via tantivy + structural heuristics), 0–100 with per-signal breakdown.
- GEO scoring using free public signals only (entity coverage, Q&A phrasing, list phrasing, metadata completeness, anti-stuffing ceiling).
- All scores persisted with component JSON for transparency and agent consumption.

#### 5.3 Data Ingestion
- **Channels**: Official RSS feeds (ETag-cached; ~15 most-recent entries; history requires API key).
- **Video Links**: Multi-line text box → extract IDs → fetch metadata.
  - With API key: full rich metadata via free `videos.list` (batched ≤50 IDs/call, 1 unit per call).
  - Without API key: official oEmbed only (title/author/thumbnail — documented limitation).
- Idempotent upsert semantics; rich source wins on conflict (api > oembed > rss).
- All data stored in the single embedded `.db` and linked for analysis.

#### 5.4 Dashboard (Deferred)
- HTMX Dashboard with Data Analysis and Charts deferred to post-v1 (CLI tables + `--json` in v1; `serve` subcommand later).

#### 5.5 Grow Audience
- Next Ideas, Saved Ideas (draft/saved/discarded), Niches.
- A/B Test Manager (manual, post-v1).
- Retention Analyzer (Studio CSV import, post-v1).
- Keyword Rank Tracking (snapshots + trends).

#### 5.6 Performance
- Health Report, Competitor Scorecard (incl. PageRank centrality), Brand Alerts.

#### 5.7 Operations
- Channel Settings, Channel Backup, Canned Responses, Promo Materials (post-v1).
- **Thumbnail Generator** (Phase 3):
  - Raw assets in temporary `/assets`.
  - HTML + latest Tailwind CSS.
  - Immediate automatic deletion of raw assets after successful generation.
- **Backup (v1, mandatory):** `VACUUM INTO` snapshot + `integrity_check` + retention, auto-run before every batch ingest.

#### 5.8 Competitor Input
- Free-form text (Channel ID, @handle, or URL) + Video Link Ingestion.

#### 5.9 Agent & Binary Operability
- Native Binary (macOS / Linux / Windows).
- Clean CLI + `--json` structured output + documented exit codes (0/1/2/3/4/5).
- MCP server via `tursodb <db> --mcp` (9 tools) for Claude Code, Codex, OpenCode, Cursor, Harness CLI, and similar agents.

#### 5.10 Data Source Policy
- Always available: Official RSS + oEmbed (zero quota).
- Optional: Free YouTube Data API (user's own key in `.env`).
  - Used only when present.
  - Heavily batched and cached (ETag; per-day quota ledger).
  - Quota usage visible via `tubeforge quota`.
  - No monetary charges possible.
- No scraping.

### 6. Technical Architecture
*(Full detail in `HLD.md` and `LLD.md`)*

**Language & Development**  
Pure Rust • Harness CLI / OpenCode • Tokio only.

**Backend**  
Single binary CLI (clap + tokio). No server, no daemon, no ports. HTMX dashboard deferred.

**Storage — Embedded Turso Database (MIT, SQLite-in-Rust)**  
- Single-file `.db`, SQLite file-format compatible — portable to/from SQLite (tested escape hatch).
- WAL journal mode (MVCC never enabled — engine limitation per issue tracker).
- Pinned release version; upgrade re-testing; watchlist of open engine issues (#7664, #7596, #7995, #7523–7529, #7800, #832).
- Backup-before-ingest enforced (VACUUM INTO + integrity_check).

**Analytics — Rust-owned**  
- BM25: tantivy crate directly (engine's FTS is beta with open ranking bugs — not used).
- Vector: brute-force cosine in Rust (sufficient at v1 scale; ANN deferred).
- Graph: adjacency + PageRank in Rust (recursive CTEs unsupported by engine).

**Thumbnail Pipeline (Phase 3)**  
HTML + latest Tailwind CSS → render → immediate `/assets` cleanup.

**Binary / Wasm + Cross-Platform**  
Native binaries for macOS (first), Linux, Windows. Wasm deferred.

**Data + Analytics Pipeline**
1. Load `.env` (detect optional API key).
2. Backup guard (VACUUM INTO + integrity_check).
3. Resolve competitors and video links.
4. Fetch RSS + (oEmbed or rich API metadata); ETag caching; quota ledger.
5. Upsert into Turso `.db` (idempotent, single transaction).
6. Update tantivy BM25 index.
7. Compute SEO/GEO scores, ideas, scorecards, alerts, and analytics datasets.
8. Output to CLI tables and/or `--json` for agents; MCP server available.

### 7. Non-Functional Requirements
- Zero monetary charges at all times.
- Optional rich metadata via free YouTube Data API (user-controlled).
- SEO/GEO first.
- CLI-first with stable JSON contracts; dashboard deferred.
- Single-file portable storage with tested SQLite escape hatch.
- Cross-platform (macOS first; Linux, Windows).
- Agent-operable (`--json`, exit codes, MCP).
- High performance (interactive CLI at 1–10k videos; no ANN needed).
- Reliability: mandatory backups, WAL mode, version pinning, integrity checks.
- Privacy, security (secrets only in `.env`), open-source readiness.

### 8. Success Metrics
- Users can generate high-quality SEO/GEO-optimized Titles, Descriptions, Tags, and Strategies (verified via transparent score breakdowns).
- Users can optionally obtain rich metadata (tags, duration, full stats) without any charges.
- `tubeforge score` returns deterministic, explainable scores with component JSON.
- System works fully with no API key (RSS + oEmbed only; documented data limits).
- Ingest is idempotent: running twice yields identical state.
- Backup round-trip passes `integrity_check` in CI; escape-hatch test opens the `.db` with rusqlite.
- Thumbnail Generator always cleans `/assets` (Phase 3).
- Binary works on macOS first; Linux/Windows targets configured.
- CLI is agent-operable: documented exit codes + JSON envelope; MCP server functional.

### 9. Implementation Phases
*(Component-level detail in `HLD.md` §12 and `LLD.md` §13)*

**Phase 0 – Skeleton + Engine Gate — ✅ COMPLETE (Aug 3, 2026)**  
Repo, `.env.example`, Turso `=0.7.2` + tantivy `=0.26.1` pinned, **M4 smoke gate PASSED 5/5** (CRUD/WAL/backup round-trip/integrity_check/rusqlite-open; FTS probe confirmed engine FTS unavailable → tantivy-direct validated), CLI skeleton (`init`/`ingest links`/`backup`/`quota`), error taxonomy + exit codes. **Next: Phase 1 (Foundation).**

**Phase 1 – Foundation — ✅ COMPLETE (Aug 3, 2026)**  
Fetch (RSS/oEmbed/API+quota ledger), full v1 schema + migration runner, Ingest (idempotent upsert, source precedence, backup guard, ingest_log), tantivy index + `reindex`, CLI: `ingest channels`/`ingest links`/`refresh`/`score`(basic)/`backup`/`quota`/`mcp`, 35/35 tests green, clippy clean, live RSS smoke verified (15 videos, Google for Developers). **Next: Phase 2 (SEO/GEO Intelligence + Analytics).**

**Phase 2 – SEO/GEO Intelligence + Analytics — ✅ COMPLETE (Aug 4, 2026)**  
Full scoring engine (10 SEO + 7 GEO components, env-configurable weights, defaults sum 1.0; new free signals `location_signal` from `recordingDetails`, `topic_relevance` from `topicDetails.topicCategories`), analytics (PageRank graph, Next Ideas, keyword rank tracking, scorecard/health/alerts), CLI `ideas`/`keywords`/`scorecard`/`health`/`alerts` + `--json` envelopes, ingest hardening (bare-ID checksums, extended URL-form parsing, 32-category map, disabled-metric heuristic), migration 002 (SCHEMA_VERSION 1→2), 89/89 tests green, clippy clean. **Next: Phase 3 (Thumbnails & Polish).**

**Phase 3 – Thumbnails & Polish — ✅ COMPLETE (Aug 4, 2026)**  
Thumbnail Generator (`tubeforge thumbnail render|list-templates`; HTML + Tailwind CSS v4 templates, rendered by headless Chromium via **chromiumoxide 0.9.1** + pinned fetcher-downloaded Chromium into `<TUBEFORGE_DATA_DIR>/chromium/`, 1280×720 PNG; **mandatory `/assets` cleanup** via RAII Drop guard + error-path cleanup, `--keep-assets` debug-only) — resolves the §11 HTML-to-image open question, `check availability` (batched `videos.list` part=snippet,status; missing IDs → `video_unavailable` alerts; `privacy_status` column via migration 003, SCHEMA_VERSION 2→3, health `privacy` census), `export` (`--format zip|dir`; manifest.json + videos.csv 19 cols + channels/tags/keywords/keyword_rankings CSVs + JSON arrays; zip crate 8.6, deterministic), `filmot get` (opt-in `TUBEFORGE_FILMOT_KEY`, raw JSON passthrough, no DB writes, third-party service), agent interface hardening (stdout/stderr separation, `list_alerts(0)` LIMIT fix, nested `check availability`, `all_ideas()`, `tests/agent_contract.rs` 11 binary-level `--json` contract tests), **135/135 tests + 1 ignored (Chromium-gated render), clippy clean**. **Next: Phase 4 (Hardening & Release).**

**Phase 4 – Hardening & Release**  
**IN PROGRESS (Aug 4, 2026)** — done: performance smoke gate built + **PASSED on M4** (5k videos: ingest 20.2s + reindex 0.2s + ideas 0.06s = 20.5s vs 30s budget, release profile, `cargo test --release --test perf_gate -- --ignored`); release prep (LICENSE-MIT/LICENSE-APACHE, Cargo.toml metadata incl. rust-version 1.85 + license field, CHANGELOG 0.1.0, GitHub Actions CI matrix macOS-14/ubuntu/windows: build+clippy+test, README command table + cross-platform notes); cargo audit clean; cargo deny clean. **Remaining:** user actions (create GitHub repo, set remote, push, tag v0.1.0) + fmt pass (deferred, optional).
Performance, documentation, cross-platform testing (Linux, Windows), public open-source release of **TubeForge** (MIT/Apache-2.0).

### 10. Assumptions & Risks
**Assumptions**  
Official RSS, oEmbed, and free YouTube Data API remain available • User understands the free daily quota (no monetary charge) • Turso Database remains MIT-licensed and pre-1.0 as documented • RSS remains an undocumented best-effort endpoint (~15 recent entries).

**Risks & Mitigations**  
- **Turso pre-1.0 storage bugs** (open issues #7664 corruption with FTS+vectors, #7995 WAL epoch loss, #7596 MVCC corruption) → WAL mode only; no Turso FTS/vector index modules; backup before every batch ingest; version pinning + watchlist; rusqlite escape hatch (same `.db`).
- **Turso FTS beta ranking bugs** (#7523–7529) → BM25 computed via tantivy in TubeForge's own code; never trust engine FTS for ranking.
- API quota exhaustion → `tubeforge quota` warning + automatic fallback to RSS + oEmbed.
- Limited oEmbed data when no key is present → Document clearly.
- Same-connection concurrent writers unsupported (SQLITE_BUSY) → sequential pipeline, single writer by design.
- Recursive CTEs unsupported → graph analytics in Rust (PageRank).
- Cross-platform differences → Early testing; macOS first.
- Scope creep → Strict phase gating; dashboard/Wasm/GNN deferred.

### 11. Open Questions
- ~~Exact on-disk format~~ → **Resolved:** Turso/SQLite `.db`, single file.
- ~~Preferred lightweight charting approach~~ → **Deferred** with dashboard.
- ~~Preferred HTML-to-image method for thumbnails (SVG+resvg vs headless Chromium)~~ → **Resolved (Aug 4, 2026):** headless Chromium via **chromiumoxide 0.9.1** — literal HTML+Tailwind v4 rendering, Blink determinism, chromiumoxide_fetcher-pinned Chromium (no system Chrome dependency), MIT/Apache-2.0, actively maintained.
- ~~Degree of Wasm support in v1~~ → **Resolved:** deferred post-v1.
- ~~Default local UI authentication~~ → **Not applicable:** CLI-only v1.
- ~~SEO/GEO scoring spec~~ (signal weights/formulas — §5.2) → **Resolved (Aug 4, 2026):** documented defaults baked in (10 SEO + 7 GEO components, each set sums 1.0) with per-component env overrides (`TUBEFORGE_SEO_*` / `TUBEFORGE_GEO_*`).
- ~~Exact Turso/tantivy version pins~~ → **Resolved:** turso `=0.7.2`, tantivy `=0.26.1` (decided at the Phase 0 gate, Aug 3 2026).

---

**Project Name for Open-Sourcing:** **TubeForge**

This is the complete Product Requirements Document (Version 3.9 — Refined).

**v3.10 update (Aug 3, 2026):** Implementation status — **Phase 0 ✅ and Phase 1 ✅ complete** (engine gate 5/5, 35/35 tests, clippy clean, live RSS smoke verified). Next: **Phase 2 — SEO/GEO Intelligence + Analytics.**

**v3.11 update (Aug 4, 2026):** Implementation status — **Phase 2 ✅ complete** (full SEO/GEO scoring engine with 2 new GEO signals, analytics suite, 5 new CLI commands, ingest hardening from MW Metadata research — ID checksums, category map, disabled-metric heuristic — 89/89 tests, clippy clean). Next: **Phase 3 — Thumbnails & Polish.**

**v3.12 update (Aug 4, 2026):** Implementation status — **Phase 3 ✅ complete** (thumbnail generator via chromiumoxide + pinned Chromium with mandatory `/assets` cleanup, `check availability` + `privacy_status` migration 003, `export` with CSVs/ZIP, Filmot opt-in recovery, agent interface hardening — 135/135 tests + 1 ignored Chromium-gated, clippy clean). Next: **Phase 4 — Hardening & Release.**

**v3.13 update (Aug 4, 2026):** Implementation status — **Phase 4 — IN PROGRESS** — performance smoke gate **PASSED on M4** (5k videos: ingest 20.2s + reindex 0.2s + ideas 0.06s = 20.5s vs 30s budget, release profile); release prep complete (LICENSE-MIT/LICENSE-APACHE, Cargo.toml metadata, CHANGELOG 0.1.0, CI matrix macOS-14/ubuntu/windows, README command table + cross-platform notes); cargo audit + cargo deny clean; 136/136 tests + 2 ignored (perf gate + Chromium render), clippy clean. Remaining: user actions (repo creation, push, tag v0.1.0) + optional fmt pass.

**Key updates introduced in v3.9 (retained):**
- **Storage:** custom "from scratch" database replaced with embedded **Turso Database** (MIT, SQLite-in-Rust): single-file `.db`, SQLite-compatible, portable back to SQLite (tested escape hatch).
- **Analytics:** BM25 via **tantivy** in TubeForge's own code; vector via brute-force cosine; graph via PageRank — all Rust-owned, none of it depending on the engine's experimental index modules (beta ranking bugs, open corruption issue #7664).
- **Reliability:** WAL mode only; mandatory backup (VACUUM INTO + integrity_check) before every batch ingest; version pinning + engine-issue watchlist.
- **Interface:** CLI-first v1 with `--json` envelope, documented exit codes, and MCP server; HTMX dashboard deferred.
- **Cross-platform:** macOS (M4) first-class; Linux and Windows in Phase 4.

Implementation status: **Phases 0–3 delivered.** You can now begin **Phase 4**.
