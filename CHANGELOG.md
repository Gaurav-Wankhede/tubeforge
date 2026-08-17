# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Greedy Bot** — autonomous topic research engine that discovers and researches YouTube topics using the channel's own data. Five data sources (competitor tags, channel tags, tracked keywords, suggested tags, related keywords) feed an auto-seed pipeline (`greedy seeds init`). Topic candidates are generated from autocomplete suggestions, competitor tags, related keywords, and seed drift — deduplicated against research history with a 24h cooldown. Results are persisted to `greedy_research_history` / `greedy_topic_log` / `greedy_seeds` tables (schema v10, 25 tables total). Commands: `greedy run`, `greedy status`, `greedy seeds add|list|deactivate|init`, `greedy daemon`, `greedy stop`. Daemon mode runs on a configurable interval with PID file management and graceful SIGINT/SIGTERM shutdown.
- **Engine independence (v4.0)** — storage moved to a from-scratch embedded
  **`tfdb`** engine (`.wal` + `.dat`, fsync-on-commit WAL + atomic checkpoint,
  pure Rust, no SQLite/SQL, no external database); **own BM25** engine replaces
  tantivy; **raw-Hyper** web framework replaces Axum; **SSE** replaces htmx
  polling; **WebSocket JSON-RPC** method surface.
- **Stdio JSON-RPC agent bridge** — `tubeforge rpc` connects agent harnesses
  (OpenCode, Claude Code, Codex, Hermes, Pi Agent) over line-delimited JSON-RPC
  on stdin/stdout (same method surface as the dashboard `/ws`). Replaces the
  removed `mcp`/`tursodb` external server.
- **Content layer** — `analyze`, `transcript`, `metadata`, `comments`, `gaps`,
  `forecast`, `suggest`, `tags`, `videos dedupe`; growth forecasting (weighted
  OLS on `channel_snapshots`); packaging-psychology title formulas.
- **Scoring** — extended to **18 SEO** components (10 structural + 5 vidIQ +
  3 graph via `graph_scores`) + 7 GEO + packaging-psychology supporting layer.
- **Knowledge Graph** — kg_entities/kg_relations/kg_communities tables, PageRank
  + Louvain, `graph_scores` on existing endpoints (internal-only; no `/api/kg/*`).

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
