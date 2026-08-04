# TubeForge — High-Level Design (HLD)

**Project:** TubeForge — local-first YouTube SEO/GEO growth engine
**Document version:** 1.2 | **Date:** August 4, 2026
**Status:** Approved — Phases 0–3 delivered; basis for Phase 4
**Companion documents:** `PRD.md` (v3.12), `LLD.md`

---

## 1. Executive Summary

TubeForge is a **single-binary, CLI-first, fully-local** tool that ingests YouTube data from free sources (RSS, oEmbed, optional user-provided YouTube Data API v3 key), stores it in an **embedded Turso Database** (a from-scratch SQLite rewrite in Rust, MIT-licensed, single-file `.db` format), and produces **SEO/GEO-optimized Titles, Descriptions, Tags, and Video Strategies** plus competitor analytics — all computed in Rust with zero external processes, zero monetary charges, and zero scraping.

All heavy lifting that must be *correct* (BM25 ranking, vector similarity, graph analytics) is owned by TubeForge's own Rust code using proven libraries (tantivy), rather than depending on the upstream engine's experimental index modules.

---

## 2. Goals & Non-Goals

### Goals (v1)
- CLI-only workflow operable by humans and AI agents (Claude Code, Codex, OpenCode, Cursor, Harness).
- Ingest channels (RSS) and video links (oEmbed + optional API) in bulk.
- Generate and score Titles, Descriptions, Tags with transparent SEO + GEO scoring.
- Next Ideas, Keyword Rank Tracking, Competitor Scorecard, Health Report, Brand Alerts.
- Thumbnail Generator (Phase 3) with mandatory `/assets` cleanup.
- Every secret in `.env`; zero monetary charges; zero scraping.
- Single-file, SQLite-compatible, portable storage with a tested escape hatch.
- Mac mini M4 (macOS arm64) primary target; Linux/Windows later.

### Non-Goals (v1)
- HTMX dashboard (deferred; `serve` subcommand post-v1).
- Wasm build (deferred; possible later — Turso has Wasm bindings, FTS needs opt-in `wasm-fts`).
- ANN vector indexing (Turso roadmap item #832; unnecessary at 1–10k rows).
- ML/GNN models (graph analytics via PageRank-class algorithms; GNN deferred indefinitely).
- Multi-process / concurrent-writer support (Turso deliberately errors `SQLITE_BUSY` on same-connection concurrent writers; pipeline is sequential by design).
- MVCC journal mode (Turso rejects custom index modules in MVCC; WAL is the locked mode).
- Cloud sync, SaaS, multi-tenancy.

---

## 3. Context Diagram

```
                        ┌──────────────────────────┐
                        │      YouTube (external)  │
                        │  • Channel RSS feeds     │  zero quota
                        │  • oEmbed endpoint       │  zero quota
                        │  • Data API v3 (opt-in)  │  user's free key, 10k units/day
                        └───────────┬──────────────┘
                                    │ HTTPS (reqwest, tokio)
┌───────────────────────────────────▼──────────────────────────────────┐
│                     USER MACHINE (macOS arm64)                        │
│                                                                       │
│   ┌──────────────────────────────────────────────────────────────┐   │
│   │               tubeforge — single Rust binary                  │   │
│   │                                                              │   │
│   │  Human (terminal)          AI agents (--json / MCP)          │   │
│   │        │                           │                          │   │
│   │        ▼                           ▼                          │   │
│   │  ┌───────────────────────────────────────────┐                │   │
│   │  │            CLI / Interface Layer           │                │   │
│   │  │  clap dispatch · output.rs · error codes   │                │   │
│   │  └──────────────────┬────────────────────────┘                │   │
│   │                     ▼                                          │   │
│   │  ┌────────────┐ ┌─────────────┐ ┌──────────────────────────┐  │   │
│   │  │ Fetch Layer│→│ Ingest Layer│→│ Scoring & Analytics Layer│  │   │
│   │  │ RSS/oEmbed │ │ resolve,    │ │ SEO · GEO · BM25(tantivy)│  │   │
│   │  │ /API batch │ │ dedupe,     │ │ cosine · PageRank · ideas│  │   │
│   │  │ quota/cache│ │ upsert, log │ │ scorecards · alerts      │  │   │
│   │  └────────────┘ └──────┬──────┘ └───────────┬──────────────┘  │   │
│   │                        ▼                    ▼                  │   │
│   │  ┌────────────────────────────────────────────────────────┐   │   │
│   │  │                  Storage Layer                          │   │   │
│   │  │  Turso DB (single .db, WAL) · tantivy index dir ·      │   │   │
│   │  │  backup service (VACUUM INTO + integrity_check)        │   │   │
│   │  └────────────────────────────────────────────────────────┘   │   │
│   │                                                               │   │
│   │  config: ~/.tubeforge/.env · data: ~/.tubeforge/*             │   │
│   └───────────────────────────────────────────────────────────────┘   │
└───────────────────────────────────────────────────────────────────────┘
```

---

## 4. Component Responsibilities

| Layer | Component | Responsibility |
|---|---|---|
| Interface | `cli` | clap subcommand parsing, global flags (`--json`, `--verbose`, `--db-path`), exit codes |
| Interface | `output` | human tables vs. JSON envelope rendering; stdout/stderr discipline |
| Interface | MCP | external `tursodb --mcp` for agents (9 tools); no in-process MCP server in v1 |
| Fetch | `rss` | Channel RSS fetch + parse (title, desc, published, views, rating, thumb, link) |
| Fetch | `oembed` | Single-video metadata (title, author, thumbnail) — no-key fallback |
| Fetch | `api` | YouTube Data API v3 client — `videos.list` batched ≤50 IDs/call, 1 unit/call; rich metadata; never `search.list` (separate 100/day bucket) |
| Fetch | `quota` | Per-endpoint budget accounting, persisted usage, dashboard output |
| Ingest | `ingest` | URL/ID extraction, @handle → channel_id resolution, dedupe, transactional upsert, ingest log, backup guard |
| Storage | `db` | Turso repository layer (schema, migrations, CRUD) — the only module touching the engine |
| Storage | `index` | tantivy BM25 index over titles/descriptions/tags; rebuildable (`reindex`) |
| Storage | `backup` | `VACUUM INTO` snapshot + `integrity_check` + retention; auto-run before batch ingest |
| Analytics | `scoring` | SEO score (BM25 signals + title/desc/tag heuristics) + GEO score (free signals) → 0–100 composite, components JSON |
| Analytics | `graph` | Competitor edges → PageRank-style centrality; Next Ideas ranking |
| Analytics | `reports` | scorecard, health report, keyword rank tracking, brand alerts |
| Config | `config` | `.env` loading, `TUBEFORGE_DB_PATH` resolution, weights overrides |

---

## 5. Key Data Flows

### 5.1 Ingest pipeline (channels)
```
tubeforge ingest channels @handle1 @handle2
  → backup guard (auto VACUUM INTO + integrity_check)
  → resolve channel_ids (RSS lookup / API channels.list if key present)
  → fetch RSS per channel (ETag cache; 403/quota → oEmbed fallback path)
  → [optional key] batch videos.list (≤50 ids/call) → rich metadata
  → upsert videos + channels (single Turso transaction)
  → update tantivy index (add/delete documents)
  → recompute scores for changed videos
  → write ingest_log; print summary (table or --json)
```

### 5.2 Scoring pipeline
```
draft or stored video (title, desc, tags, channel, date)
  → tantivy BM25 queries (title/desc/tag corpora)          [SEO lexical signals]
  → heuristic components (length, front-loading, density,   [SEO structural signals]
     FTA, hashtags, tag count/order)
  → GEO free signals (entity coverage, Q&A phrasing,        [GEO signals]
     list/how-to phrasing, conversational tone)
  → weighted composite 0–100 + per-component breakdown
  → persist scores; feed ideas/keywords/scorecard
```

### 5.3 Backup & recovery
```
backup → VACUUM INTO backup/<ts>.db → PRAGMA integrity_check
       → keep last N (config) → prune
restore → open backup .db (drop-in path) or copy over main
reindex → rebuild tantivy from videos table (idempotent)
escape → open same .db with rusqlite (COMPAT guarantee #1)
```

### 5.4 Agent flow
```
agent → tubeforge score --draft-title "..." --json  → structured envelope
agent → tursodb ~/.tubeforge/tubeforge.db --mcp     → 9 MCP tools
```

---

## 6. Data Source Policy (locked)

| Source | Availability | Cost | Richness | Role |
|---|---|---|---|---|
| Channel RSS | Always | 0 | title, desc, published, views, rating, thumb (~15 recent) | Baseline for channels |
| oEmbed | Always | 0 | title, author, thumbnail only | Baseline for single videos |
| YouTube Data API v3 | Only with user's key in `.env` | 10,000 units/day; `videos.list` = 1 unit/call (≤50 IDs) | tags, category, duration, full stats | Rich metadata, batched + cached |
| Scraping | **Never** | — | — | Explicitly forbidden (ToS) |

On quota exhaustion: automatic fallback to RSS/oEmbed + warning (`tubeforge quota` shows state). `search.list` avoided by design (separate 100-calls/day bucket).

---

## 7. Storage Architecture

### 7.1 Turso Database (embedded engine)
- **Why:** MIT license (PRD's "open-source friendly" holds with zero disclosures); true single-file `.db`; SQLite file-format compatible — portable to/from SQLite; from-scratch Rust rewrite of SQLite (the "custom DB in Rust" ambition, maintained upstream); built-in MCP server; Wasm path later; production users + their own FAQ "keep independent backups".
- **Mode:** WAL, **not** MVCC (Turso issue #7800: FTS/vector index modules rejected in MVCC; #7596: MVCC corruption bug).
- **Version policy:** pin released version; re-test on upgrade; watchlist issues #7664, #7596, #7995, #7523–7529, #7800, #832.

### 7.2 What Turso does NOT provide in v1 (and why)
- **Turso FTS (tantivy-based):** self-declared beta with open ranking bugs (#7524 ASC order wrong, #7526 LIMIT wrong, #7523 OFFSET wrong, #7528 fallback divergence) and a corruption bug when combined with vectors (#7664, high-priority, open). → TubeForge uses **tantivy directly in Rust** for BM25.
- **Vector ANN:** roadmap only (#832). → brute-force cosine in Rust (1–5 ms at 1–10k rows).

### 7.3 Escape hatch
COMPAT.md guarantee: *"You should always be able to go back to SQLite."* If storage corruption/loss of the #7664/#7995 class occurs, swap the `db` module for rusqlite against the same `.db` — no data migration, no schema change.

---

## 8. Interface Architecture

| Interface | Mechanism | Consumers |
|---|---|---|
| CLI | `tubeforge <cmd> [flags]` | Humans |
| Structured output | `--json` envelope (ok/data/meta/error) + documented exit codes | AI agents, scripts |
| MCP | `tursodb <db> --mcp` (9 tools) | Claude Code, Cursor, etc. |
| Config | `.env` (never flags for secrets) | All |

---

## 9. Deployment & Topology

- **Single process, zero ports, zero daemons.** No server, no DB process, no browser.
- Data root: `~/.tubeforge/` → `tubeforge.db`, `index/` (tantivy), `backups/`, `.env`.
- Override: `TUBEFORGE_DB_PATH`.
- Target: macOS arm64 (M4) first-class; Linux x86_64/arm64; Windows later (Turso cross-platform; note simsimd/aegis C deps compile on macOS without extra tooling).

---

## 10. Non-Functional Requirements

| NFR | Requirement | How met |
|---|---|---|
| Reliability | No silent data loss | Pre-ingest backup (VACUUM INTO + integrity_check); WAL mode; pinned version; watchlist; rusqlite escape |
| Performance | Interactive CLI (<2s typical) | 1–10k row corpus; tantivy <10ms startup; brute-force cosine in ms; no ANN needed |
| Security | Local-only secrets | `.env` only; no network listeners; YouTube API key never logged |
| Privacy | All data local | No telemetry; no cloud |
| Portability | Data readable elsewhere | SQLite `.db` format; `--json` output |
| Agent-operability | Deterministic machine interface | JSON envelope, documented exit codes, MCP |
| Open-source readiness | MIT/Apache-2.0 clean | All deps permissive: Turso MIT, tantivy MIT, tokio MIT, clap MIT/Apache-2.0 |
| Observability | Diagnosable failures | `--verbose`, structured error codes with source context, ingest_log table |

---

## 11. Risks & Mitigations (from GitHub issues research, Aug 3 2026)

| # | Risk (evidence) | Mitigation |
|---|---|---|
| R1 | Storage corruption with FTS+vectors (#7664, high-priority, open) | No Turso FTS/vector modules in v1; tantivy/brute-force in Rust; backup before ingest |
| R2 | WAL epoch loss on crash pre-checkpoint (#7995, fix in open PR) | Backup before ingest; `integrity_check`; WAL mode; version pin |
| R3 | MVCC-mode corruption + index-module rejection (#7596, #7800) | WAL mode locked; never enable MVCC |
| R4 | FTS ranking bugs (#7523–7529, beta) | tantivy-direct; own scoring tests |
| R5 | Pre-1.0 API churn | Pin versions; re-test on upgrade; watchlist |
| R6 | Same-connection concurrent writers → SQLITE_BUSY | Sequential pipeline by design; single writer |
| R7 | Recursive CTEs unsupported (COMPAT.md) | Graph analytics in Rust (PageRank), not SQL |

---

## 12. Phase → Component Map

| Phase | Delivers |
|---|---|
| 0 | ✅ COMPLETE — Repo, config, M4 smoke gate (Turso CRUD/WAL/backup round-trip), CLI skeleton, error taxonomy |
| 1 | ✅ COMPLETE — Fetch (RSS/oEmbed/API+quota), Ingest, Storage (schema, migrations), tantivy index, backup, CLI: ingest/score(basic)/backup/quota, MCP integration |
| 2 | Scoring engine (SEO+GEO), ideas, keywords, scorecard, health, alerts, graph analytics |
| 3 | ✅ COMPLETE (Aug 4, 2026) — Thumbnail generator (HTML→image via chromiumoxide headless Chromium + `/assets` cleanup), `check availability` (privacy census, migration 003), `export` (CSV/ZIP), Filmot opt-in recovery, agent interface hardening |
| 4 | Hardening, docs, cross-platform, release (MIT) |

---

## 13. ADRs (decision records — full context in session research)

| ADR | Decision | Alternatives rejected |
|---|---|---|
| ADR-1 | Embedded **Turso Database** | From-scratch custom DB (scope bomb); SurrealDB (BUSL, directory storage, Windows/Wasm limits); rusqlite-only (no Rust-native engine, no MCP, less aligned with PRD vision) |
| ADR-2 | **BM25 via tantivy crate directly** (not Turso FTS) | Turso FTS (beta ranking bugs, #7664 corruption combo); SQLite FTS5 (wrong surface for Turso) |
| ADR-3 | **Vector = brute-force cosine in Rust** | Turso vector ANN (doesn't exist, #832); sqlite-vec (pre-v1 alpha) |
| ADR-4 | **Graph = Rust adjacency + PageRank** | SQL recursive CTEs (unsupported); GNN (no requirement) |
| ADR-5 | **WAL mode, never MVCC** | MVCC (#7596 corruption, #7800 index rejection) |
| ADR-6 | **CLI-only v1** | HTMX dashboard (deferred) |
| ADR-7 | **Backup before every batch ingest** (VACUUM INTO + integrity_check) | Trusting pre-1.0 durability |
| ADR-8 | **MCP via tursodb CLI** (external) | In-process MCP server (scope) |
| ADR-9 | **No embeddings in v1** (lexical BM25 + token overlap only) | Embedding pipelines (scope; revisit post-v1) |

---

## 14. Open Questions (HLD level)

1. SEO/GEO scoring spec (§5.2 of PRD) — signal weights/formulas to be authored with the user (the one item research cannot settle).
2. ~~Thumbnail HTML→image method (Phase 3): SVG+resvg vs headless Chromium.~~ → **Resolved (Aug 4, 2026):** headless Chromium via **chromiumoxide 0.9.1** (CDP) with chromiumoxide_fetcher-pinned Chromium (auto-downloaded to `<data>/chromium`, `TUBEFORGE_CHROMIUM_DIR`); Tailwind v4 via standalone CLI (no Node); rationale — literal HTML+Tailwind v4 rendering, Blink determinism, pinned browser (no system Chrome), MIT/Apache-2.0, actively maintained.
3. ~~Exact Turso crate + tantivy version pins~~ → **Resolved at Phase 0 gate:** turso `=0.7.2`, tantivy `=0.26.1`.
4. Windows support timing (v1 macOS-first).
