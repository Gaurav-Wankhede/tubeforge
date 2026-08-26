# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.1.0] - 2026-08-26

### Fixed & Optimized

- **Knowledge Graph Performance & Batching (74.6x Speedup)**:
  - Replaced single-row database writes with atomic batch persistence (`persist_kg_batch`), reducing Knowledge Graph construction and persistence time from **4m40s (280s)** down to **3.75s** across 1,644 entities and 159,148 relations.
  - Added `load_or_build()` cache validation to eliminate redundant graph rebuilds on RPC requests.
- **Graph-Aware Multi-Dimensional Scoring Calibration**:
  - Calibrated PageRank probability distribution scaling ($P \times |V| \times 25.0$) to produce granular, non-zero percentiles ($0\text{--}100$) for `tag_authority`, `topic_dominance`, and `keyword_competition`.
  - Added multi-candidate tag and keyword resolution to prevent string formatting mismatches.
- **Atomic Database Checkpointing in `tfdb`**:
  - Added automatic `eng.checkpoint()?` to all keyword, ranking, score, and research mutation functions (`add_keywords`, `upsert_ranking`, `upsert_score`, `upsert_keyword_research`), preventing unpersisted WAL data loss on subsequent reads.
- **Full Corpus Batch Recalculation Engine**:
  - Added `upsert_scores_batch` in `db_tf.rs` for lightning-fast multi-row score transactions.
  - Extended `scores.backfill` RPC method with `force: true` parameter, recalculating all 1,065 videos across the catalog in **$< 6\text{ seconds}$**.
- **Keyword Ranking Engine Synchronization**:
  - Integrated 14 production target keywords into the live ranking engine.
  - Calculated exact ranking positions, deltas, and SERP competition against own vs competitor videos.
- **100% Test Suite Verification**:
  - All 319 unit, integration, and property tests passing cleanly.
- **`unwrap_or*` Default-Value Audit (Rust Best Practices)**:
  - Eliminated all 15 eager-evaluation sites flagged by Clippy `or_fun_call` across the analytics, serve, and command layers — expensive fallbacks (`Utc::now()`, JSON object/array construction, string clones, `format!` titles) now evaluate lazily via `unwrap_or_else` closures, keeping allocation off the happy path.
  - **Correctness fix**: unparsable channel `fetched_at` timestamps fell back to `Utc::now()`, silently hiding stale channels from health checks and alerts; they now fall back to `UNIX_EPOCH`, so unknown freshness surfaces as stale (fail-safe).
  - **Error visibility**: `videos dedupe` no longer swallows database count errors (`unwrap_or(0)` → `?`); failed keyword-recency probes and corpus-resonance reads in `analyze` now emit `tracing::warn` diagnostics instead of silently reading as zero signal; `backup` logs snapshot stat failures with path and cause.
  - Kanban default ticket titles are allocated only when no `--title` override is given, and follow the house no-colon title style (`keyword — Visual Breakdown & Mental Model`).
  - Unified `home_dir()` fallback in config loading (`.`, matching `Config::defaults()`, instead of an empty `PathBuf`).
- **Test Suite Restoration**:
  - Added the missing `niche_terms` field to `Config` initializers in the `phase1`–`phase3` and `perf_gate` integration suites; removed dead imports from `real_kg_benchmark`. All targets now compile warning-free: 319 lib tests, 18 serve tests, and 30 phase tests passing.

## [0.2.0] - 2026-08-25

### Added

- **Built-in Kanban Ticket System (`kanban` command)** — Full TODO and roadmap lifecycle management for future video production. Directly interlinks with TubeForge's research corpus (`keyword_research`, competitor SERPs, suggested tags) without duplicating data.
  - Table `kanban_tickets` added to `tfdb_schema` (26 tables total).
  - Subcommands: `kanban create`, `kanban from-research`, `kanban list`, `kanban move`, `kanban show`, `kanban delete`, `kanban prompt`.
  - Supports dual-channel taxonomy (`TECHVERSE` and `BOOKVERSE`) and state transitions (`todo` ➔ `inprogress` ➔ `done` ➔ `published`) with YouTube URL and video ID attachment.
  - Generates First-Screen contract production blueprints automatically from research data.
- **Contextual Multi-Armed Bandit (LinUCB) Engine** (`analytics/bandit.rs`) — Linear UCB arm scoring with online ridge regression and Sherman-Morrison updates for optimal title/thumbnail variant selection under uncertainty.
- **Loewenstein Gap & Threat Prevention Scorer** (`scoring/psych.rs`) — Behavioral psychology title scoring for definite referring expressions, curiosity gaps, and loss-aversion patterns.
- **First-Screen Retention Contract Bridge** (`commands/prompt.rs`) — AI prompt generator enforcing the 0:00–1:00 retention contract (0:00–0:15 Hook ➔ 0:15–0:35 Payoff ➔ 0:35–1:00 Vehicle).
- **Tufte Data-Ink Thumbnail Generator** (`templates/default.html`) — Pure black `#000000` radical simplicity thumbnail template with single left focal mark and bold 2-line headline.
- **Greedy Bot** — autonomous topic research engine that discovers and researches YouTube topics using the channel's own data. Five data sources feed an auto-seed pipeline. Commands: `greedy run`, `greedy status`, `greedy seeds add|list|deactivate|init`, `greedy daemon`, `greedy stop`.
- **Engine independence (v4.0)** — storage moved to a from-scratch embedded **`tfdb`** engine (`.wal` + `.dat`, fsync-on-commit WAL + atomic checkpoint, pure Rust, no SQLite/SQL); **own BM25** engine; **raw-Hyper** server; **SSE** real-time updates.
- **Stdio JSON-RPC agent bridge** — `tubeforge rpc` connects agent harnesses over line-delimited JSON-RPC.
- **Content layer** — `analyze`, `transcript`, `metadata`, `comments`, `gaps`, `forecast`, `suggest`, `tags`, `videos dedupe`.
- **Scoring** — extended to **18 SEO** components + 7 GEO + packaging-psychology supporting layer.
- **Knowledge Graph** — `kg_entities`/`kg_relations`/`kg_communities` tables, PageRank + Louvain clustering.

## [0.1.0] - 2026-08-04

Initial public release — Phases 0–3 of the TubeForge roadmap:

### Added

- **Engine gate** — single-crate Rust CLI (`tubeforge`), tokio async runtime,
  embedded `tfdb` engine (from-scratch crash-safe store — `.wal` + `.dat`,
  pure Rust, no SQLite/SQL), stable `{ok,data,meta,error}` JSON envelope
  (LLD §4.2) with `--json` on all commands.
- **Ingest** — `ingest channels` (RSS, ~15 most-recent videos, ETag-cached) and
  `ingest links` (oEmbed, no API key needed); optional YouTube Data API v3
  enrichment with batched `videos.list` (≤50 ids/call, 1 unit/call) and a
  per-day quota ledger (`quota` command, reset at midnight PT, `WARN_AT`
  threshold).
- **Schema & migrations** — versioned schema (SCHEMA_VERSION 1→3), backup +
  `refresh` ETag-aware updates (304 → no writes, no snapshot).
- **Scoring** — own BM25 title/description/tag index (`reindex`, idempotent),
  `score --draft-title` envelope with SEO and GEO composites, weights
  env-configurable (`TUBEFORGE_WEIGHTS_*`, per-component overrides).
- **Analytics** — `ideas`, `keywords`, `scorecard`, `health`, `alerts`;
  PageRank-influenced idea ranking, keyword ranks, stale-channel rules.
- **Ingest hardening** — ID checksums, category map, disabled-metric heuristic.
- **Thumbnail generator** — `thumbnail render|list-templates`, headless
  Chromium via chromiumoxide 0.9.1 + pinned fetcher-downloaded Chromium
  (`TUBEFORGE_CHROMIUM_DIR`), Tailwind v4 templates, 1280×720 PNG, mandatory
  `/assets` cleanup (RAII Drop guard, `--keep-assets` debug-only).
- **Availability** — `check availability` (batched `videos.list`
  part=snippet,status; missing IDs → `video_unavailable` alerts;
  `privacy_status` column, migration 003).
- **Export** — `export --format zip|dir`: manifest.json + videos.csv (19
  cols) + channels/tags/keywords/keyword_rankings CSVs + JSON arrays
  (deterministic ZIP via `zip` crate 8.6).
- **Filmot** — `filmot get` opt-in recovery lookup (`TUBEFORGE_FILMOT_KEY`),
  raw JSON passthrough, no DB writes.
- **Agent hardening** — stdout/stderr separation, `list_alerts(0)` LIMIT fix,
  nested `check availability`, `all_ideas()`, `tests/agent_contract.rs`
  binary-level `--json` contract tests.
- **Agents** — `tubeforge rpc` stdio JSON-RPC bridge (line-delimited requests
  on stdin, responses on stdout; same method surface as the dashboard `/ws`).

### Changed

- Nothing — first release.

### Fixed

- Nothing — first release.
