# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-08-04

Initial public release — Phases 0–3 of the TubeForge roadmap:

### Added

- **Engine gate** — single-crate Rust CLI (`tubeforge`), tokio async runtime,
  embedded Turso database (single-file SQLite-compatible `.db`, WAL mode),
  stable `{ok,data,meta,error}` JSON envelope (LLD §4.2) with `--json` on all
  commands.
- **Ingest** — `ingest channels` (RSS, ~15 most-recent videos, ETag-cached) and
  `ingest links` (oEmbed, no API key needed); optional YouTube Data API v3
  enrichment with batched `videos.list` (≤50 ids/call, 1 unit/call) and a
  per-day quota ledger (`quota` command, reset at midnight PT, `WARN_AT`
  threshold).
- **Schema & migrations** — versioned schema (SCHEMA_VERSION 1→3), backup +
  `refresh` ETag-aware updates (304 → no writes, no snapshot).
- **Scoring** — tantivy BM25 title/description index (`reindex`, idempotent),
  `score --draft-title` envelope with SEO (10 components) and GEO (7
  components) composites, weights env-configurable (`TUBEFORGE_WEIGHTS_*`,
  per-component overrides).
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
- **MCP** — `tubeforge mcp` prints a `.mcp.json`-compatible snippet pointing
  at `tursodb <db> --mcp` (external MCP server, ADR-8).

### Changed

- Nothing — first release.

### Fixed

- Nothing — first release.
