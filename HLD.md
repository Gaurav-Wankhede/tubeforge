# TubeForge — High-Level Design (HLD)

**Project:** TubeForge — local-first YouTube SEO/GEO growth engine
**Document version:** 1.3 | **Date:** August 14, 2026
**Status:** Approved — Phases 0–6 delivered; Phase 4 (release) hardening
**Companion documents:** `PRD.md` (v4.0), `LLD.md`

> **v1.3 update (Aug 14, 2026):** storage, search, and server sections rewritten to reflect the engine-independence re-architecture: Turso/SQLite → `tfdb` (custom WAL+snapshot, no SQLite escape hatch), tantivy → from-scratch BM25, Axum → raw-Hyper + WebSocket JSON-RPC + SSE. Content/`analyze` layer and growth forecasting added.

---

## 1. Executive Summary

TubeForge is a **single-binary, CLI-first, fully-local** tool that ingests YouTube data from free sources (RSS, oEmbed, optional user-provided YouTube Data API v3 key, opt-in local yt-dlp), stores it in an embedded **`tfdb` engine** (a from-scratch storage engine in pure Rust with an append-only checksummed WAL + atomic checkpoint snapshot), and produces **SEO/GEO-optimized Titles, Descriptions, Tags, and Video Strategies** plus competitor analytics — all computed in Rust with zero external processes, zero monetary charges, and zero scraping.

All heavy lifting that must be *correct* (BM25 ranking, vector similarity, graph analytics) is **owned by TubeForge's own Rust code** — a from-scratch BM25 engine in `src/search`, a from-scratch HNSW module in `src/tfdb/hnsw.rs` (unwired), and PageRank/Louvain graph analytics — rather than depending on external engine index modules.

---

## 2. Goals & Non-Goals

### Goals (v1)
- CLI-only workflow operable by humans and AI agents (Claude Code, Codex, OpenCode, Cursor, Harness).
- Ingest channels (RSS) and video links (oEmbed + optional API) in bulk.
- Generate and score Titles, Descriptions, Tags with transparent SEO + GEO scoring.
- Next Ideas, Keyword Rank Tracking, Competitor Scorecard, Health Report, Brand Alerts.
- Content layer: topic `analyze`, yt-dlp transcripts/comments, growth `forecast`, packaging-psychology titles.
- Thumbnail Generator (Phase 3) with mandatory `/assets` cleanup.
- Every secret in `.env`; zero monetary charges; zero scraping.
- Single, portable, crash-safe `tfdb` store with snapshot backup + integrity re-open.
- Mac mini M4 (macOS arm64) primary target; Linux/Windows later.

### Non-Goals (v1)
- Dashboard — **delivered** as `tubeforge serve` (loopback-only, raw-Hyper + SSE + WebSocket JSON-RPC, inline SVG charts — PRD §5.4); still a local single-user server, not a multi-tenant web app.
- SQLite-format storage / rusqlite escape hatch — **removed** (ADR-1, v1.3/PRD v4.0); storage is `tfdb` only.
- Wasm build (deferred).
- ANN vector wiring (HNSW ships but embeddings are not generated; deferred post-release).
- ML/GNN models (graph analytics via PageRank-class algorithms; GNN deferred indefinitely).
- Multi-process / concurrent-writer support (`tfdb` is single-writer by design; pipeline is sequential).
- Cloud sync, SaaS, multi-tenancy.

---

## 3. Context Diagram

```
                        ┌──────────────────────────┐
                        │      YouTube (external)  │
                        │  • Channel RSS feeds     │  zero quota
                        │  • oEmbed endpoint       │  zero quota
                        │  • Data API v3 (opt-in)  │  user's free key, 10k units/day
                         │  • yt-dlp (opt-in local) │  transcripts/comments/research
                         │  • vectron-whisper     │  local Whisper GGML (shared with Vectron)
                         └───────────┬──────────────┘
                                     │ HTTPS (reqwest, tokio) / yt-dlp subprocess / whisper-rs (local, shared with Vectron)
┌───────────────────────────────────▼──────────────────────────────────┐
│                     USER MACHINE (macOS arm64)                        │
│                                                                       │
│   ┌──────────────────────────────────────────────────────────────┐   │
│   │               tubeforge — single Rust binary                  │   │
│   │                                                              │   │
│   │  Human (terminal)          AI agents (--json / stdio RPC)  │   │
│   │        │                           │                          │   │
│   │        ▼                           ▼                          │   │
│   │  ┌───────────────────────────────────────────┐                │   │
│   │  │            CLI / Interface Layer           │                │   │
│   │  │  clap dispatch · output.rs · error codes   │                │   │
│   │  └──────────────────┬────────────────────────┘                │   │
│   │                     ▼                                          │   │
│   │  ┌────────────┐ ┌─────────────┐ ┌──────────────────────────┐  │   │
│   │  │ Fetch Layer│→│ Ingest Layer│→│ Scoring & Analytics Layer│  │   │
│   │  │ RSS/oEmbed │ │ resolve,    │ │ SEO · GEO · BM25(own)    │  │   │
│   │  │ /API batch │ │ dedupe,     │ │ cosine(HNSW, unwired) ·  │  │   │
│   │  │ yt-dlp     │ │ upsert, log │ │ PageRank · Louvain · KG  │  │   │
│   │  │ quota/cache│ │ backup guard│ │ scorecards · alerts      │  │   │
│   │  └────────────┘ └──────┬──────┘ └───────────┬──────────────┘  │   │
│   │                        ▼                    ▼                  │   │
│   │  ┌────────────────────────────────────────────────────────┐   │   │
│   │  │                  Storage Layer                          │   │   │
│   │  │  tfdb engine (<path>.wal + <path>.dat) · own BM25       │   │   │
│   │  │  index.json · backup (snapshot copy + integrity re-open)│   │   │
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
| Interface | RPC | `tubeforge rpc` stdio JSON-RPC bridge for agents (OpenCode, Claude Code, Codex, Hermes, Pi Agent) — same method surface as `/ws` |
| Fetch | `rss` | Channel RSS fetch + parse (title, desc, published, views, rating, thumb, link) |
| Fetch | `oembed` | Single-video metadata (title, author, thumbnail) — no-key fallback |
| Fetch | `api` | YouTube Data API v3 client — `videos.list` batched ≤50 IDs/call, 1 unit/call; rich metadata; never `search.list` |
| Fetch | `quota` | Per-endpoint budget accounting, persisted usage, dashboard output |
| Fetch | `ytdlp` | Transcript (auto/manual captions, WebVTT→text), comments, heatmap/live stats, SERP research via yt-dlp subprocess (opt-in) |
| Fetch | `whisper` | Local Whisper ASR fallback (`vectron-whisper` crate — `whisper-rs` GGML, `symphonia`→`rubato` 16kHz mono, `normalize()`+WER, shared model cache `<data>/models/whisper/` with Vectron) — `transcript get --engine whisper|auto` when captions missing; `transcripts.source="whisper_local"` |
| Ingest | `ingest` | URL/ID extraction, @handle → channel_id resolution, dedupe, transactional upsert, ingest log, backup guard |
| Storage | `db` | `tfdb` repository layer (22 schemas, CRUD) — the only module touching the engine |
| Storage | `search` | TubeForge-owned BM25 index (`src/search`) over titles/descriptions/tags; atomic `index.json` snapshot; rebuildable (`reindex`) |
| Storage | `backup` | snapshot copy + integrity re-open + retention; auto-run before batch ingest |
| Analytics | `scoring` | SEO score (own BM25 signals + title/desc/tag heuristics) + GEO score (free signals) → 0–100 composite, components JSON; packaging-psychology formulas |
| Analytics | `graph` | Competitor edges → PageRank centrality; Louvain communities; Next Ideas ranking |
| Analytics | `kg` | Knowledge Graph (entities/relations/communities); `graph_scores`, graph ideas/gaps |
| Analytics | `reports` | scorecard, health report, keyword rank tracking, brand alerts |
| Analytics | `forecast` | weighted OLS growth forecasting from `channel_snapshots` |
| Interface | `serve` | Dashboard server (delivered, v1.3 re-architecture): raw-Hyper web framework, WebSocket JSON-RPC (`/ws`), SSE (`/events`), inline SVG charts, CSRF origin guard on POSTs; loopback-only; long-running — no JSON envelope, stdout stays empty |
| Config | `config` | `.env` loading, `TUBEFORGE_DB_PATH` resolution, weights overrides |

---

## 5. Key Data Flows

### 5.1 Ingest pipeline (channels)
```
tubeforge ingest channels @handle1 @handle2
  → backup guard (auto snapshot copy + integrity re-open)
  → resolve channel_ids (RSS lookup / API channels.list if key present)
  → fetch RSS per channel (ETag cache; 403/quota → oEmbed fallback path)
  → [optional key] batch videos.list (≤50 ids/call) → rich metadata
  → upsert videos + channels (single tfdb transaction)
  → update BM25 index (add/delete documents)
  → recompute scores for changed videos
  → write ingest_log; print summary (table or --json)
```

### 5.2 Scoring pipeline
```
draft or stored video (title, desc, tags, channel, date)
  → BM25 queries (title/desc/tag corpora)                     [SEO lexical signals]
  → heuristic components (length, front-loading, density,      [SEO structural signals]
     FTA, hashtags, tag count/order)
  → GEO free signals (entity coverage, Q&A phrasing,           [GEO signals]
     list/how-to phrasing, conversational tone, location/topic)
  → [KG built] graph components (tag_authority, topic_dominance, keyword_competition)
  → weighted composite 0–100 + per-component breakdown
  → persist scores; feed ideas/keywords/scorecard
```

### 5.3 Backup & recovery
```
backup → copy <path>.dat snapshot → backup/<ts>.db (standalone tfdb checkpoint)
       → integrity re-open (Db::open on the copy) → keep last N → prune
restore → point TUBEFORGE_DB_PATH at the checkpoint (drop-in)
reindex → rebuild BM25 from videos table (idempotent)
```

### 5.4 Agent flow
```
agent → tubeforge score --draft-title "..." --json  → structured envelope
agent → tubeforge rpc                                → stdio JSON-RPC (line-delimited)
agent → ws://127.0.0.1:PORT/ws  (WebSocket JSON-RPC, PRD §13)
```

### 5.5 Dashboard flow (serve — delivered, v1.3)
```
tubeforge serve --port 8080
  → bind 127.0.0.1:8080 (loopback only; non-loopback host → exit 2)
  → GET / → health cards + SSE counts (5s, on-change) + inline SVG charts
  → GET /events → SSE stream (EventSource; 15s : ping heartbeat)
  → WS /ws → JSON-RPC (dashboard.overview, ideas.analyze, scores.*, ...)
  → hx-post /alerts/clear (legacy HTMX) / JSON-RPC mutations → CSRF origin guard → DB mutation
  → single shared Db (single-writer caveat); stdout empty; Ctrl-C clean shutdown
```

---

## 6. Data Source Policy (locked)

| Source | Availability | Cost | Richness | Role |
|---|---|---|---|---|
| Channel RSS | Always | 0 | title, desc, published, views, rating, thumb (~15 recent) | Baseline for channels |
| oEmbed | Always | 0 | title, author, thumbnail only | Baseline for single videos |
| YouTube Data API v3 | Only with user's key in `.env` | 10,000 units/day; `videos.list` = 1 unit/call (≤50 IDs) | tags, category, duration, full stats | Rich metadata, batched + cached |
| yt-dlp | Opt-in, local | 0 | transcripts (auto/manual subs), comments, heatmap/live stats, SERP research | Content layer; subprocess |
| vectron-whisper (Whisper GGML) | Opt-in, local | 0 | private offline transcripts (16kHz mono, `normalize()`+WER), fallback when captions disabled | Offline ASR; shared model cache with Vectron |
| Filmot | Opt-in (`TUBEFORGE_FILMOT_KEY`) | 0 | raw JSON passthrough (recovery only) | No DB writes |
| Scraping | **Never** | — | — | Explicitly forbidden (ToS) |

On quota exhaustion: automatic fallback to RSS/oEmbed + warning (`tubeforge quota` shows state). `search.list` avoided by design (separate 100-calls/day bucket).

---

## 7. Storage Architecture

### 7.1 `tfdb` engine (from-scratch, pure Rust)
- **Why (ADR-1):** removes the external engine dependency entirely; MIT-clean with zero third-party storage licensing; full control over durability and schema; aligns with the "own the heavy lifting" principle.
- **On-disk:** two companion files per base path — `<path>.wal` (append-only, CRC32-checksummed transaction log) + `<path>.dat` (latest atomic checkpoint snapshot). Magics `TFWL`/`TFDT`; rows serialized via deterministic serde_json.
- **Durability:** `begin`→`Tx` stages in memory; `commit` writes one WAL record + `fsync` (default on) then applies to the live snapshot; crash replays WAL on open and truncates any torn (non-committed) tail; `checkpoint()` writes the full snapshot via temp-file + `rename` (atomic) then truncates the WAL. Committed data never lost; partial transactions never appear.
- **Concurrency:** single write lock serializes writers; reads served from the in-memory snapshot. Single-writer by design.
- **Schema:** typed `TableSchema`/`Col` (22 tables — see PRD §15), no SQL DDL; queries via Rust scans (`src/tfdb/query.rs`: `sum/avg/min/max/group_counts/join`). `SCHEMA_VERSION = 9` in `meta`.

### 7.2 Not provided in v1 (and why)
- **SQLite compatibility / rusqlite escape hatch** — removed (ADR-1). The `.dat` snapshot is fully self-contained and portable; `backup` produces a standalone tfdb checkpoint verifiable by re-open. Migration tooling to a future engine is a post-release concern.
- **ANN wiring** — HNSW module (`src/tfdb/hnsw.rs`) ships but no embeddings are generated; vector retrieval not exposed.

### 7.3 Backup & integrity
- `backup` copies the current `.dat` snapshot to `tubeforge-<ts>.db` (a standalone tfdb checkpoint), re-opens it with `Db::open` to verify integrity, prunes to keep N. Runs automatically before every batch ingest.

---

## 8. Interface Architecture

| Interface | Mechanism | Consumers |
|---|---|---|
| CLI | `tubeforge <cmd> [flags]` | Humans |
| Structured output | `--json` envelope (ok/data/meta/error) + documented exit codes | AI agents, scripts |
| Stdio JSON-RPC | `tubeforge rpc` (line-delimited JSON-RPC on stdin/stdout — same method surface as `/ws`) | Agent harnesses (OpenCode, Claude Code, Codex, Hermes, Pi Agent) |
| WebSocket JSON-RPC | `ws://127.0.0.1:<port>/ws` (PRD §13) | Dashboard, agents |
| HTTP API | `GET/POST /api/*` (PRD §5.4) | Dashboard, agents |
| SSE | `GET /events` (EventSource) | Dashboard live updates |
| Config | `.env` (never flags for secrets) | All |

---

## 9. Deployment & Topology

- **Single process, zero ports, zero daemons** (except the optional loopback dashboard server, §5.5). No DB process, no browser.
- Data root: `~/.tubeforge/` → `<db>.wal` + `<db>.dat`, `index/` (own BM25 `index.json`), `backups/`, `.env`.
- Override: `TUBEFORGE_DB_PATH`.
- Target: macOS arm64 (M4) first-class; Linux x86_64/arm64; Windows later.

---

## 10. Non-Functional Requirements

| NFR | Requirement | How met |
|---|---|---|
| Reliability | No silent data loss | Pre-ingest backup (snapshot + integrity re-open); fsync-on-commit WAL; atomic checkpoint; SCHEMA_VERSION pinning |
| Performance | Interactive CLI (<2s typical) | 1–10k row corpus; own BM25 <10ms startup; no ANN wiring needed |
| Security | Local-only secrets | `.env` only; the only network listener is the loopback-only dashboard (`serve`, 127.0.0.1, CSRF-guarded POSTs); YouTube API key never logged |
| Privacy | All data local | No telemetry; no cloud; yt-dlp/Filmot opt-in only |
| Portability | Data readable elsewhere | Self-contained `.dat` snapshot; `backup` standalone checkpoint; `--json` output |
| Agent-operability | Deterministic machine interface | JSON envelope, documented exit codes, stdio JSON-RPC, WebSocket JSON-RPC |
| Open-source readiness | MIT/Apache-2.0 clean | All deps permissive; `cargo-deny` gate |
| Observability | Diagnosable failures | `--verbose`, structured error codes with source context, ingest_log table |

---

## 11. Risks & Mitigations (updated for `tfdb`, Aug 14 2026)

| # | Risk (evidence) | Mitigation |
|---|---|---|
| R1 | From-scratch engine durability (own code) | fsync-on-commit WAL; atomic temp-file+rename checkpoint; WAL replay + torn-tail truncation; snapshot backup before every batch ingest; integrity re-open |
| R2 | No SQLite escape hatch (ADR-1) | Self-contained `.dat` snapshot; standalone `backup` checkpoint; migration tooling as post-release concern |
| R3 | Single-writer constraint | Sequential pipeline by design; snapshot readers safe; don't run `serve` alongside writing CLI commands |
| R4 | HNSW unwired (no embeddings) | BM25 lexical retrieval is the shipped path; defer embedding pipeline |
| R5 | yt-dlp external-process dependency | Gated behind opt-in flags; degrades gracefully; never page-scrapes |
| R6 | Component-count inconsistency (15 vs 18 SEO keys) | Graph components flow through `graph_scores`; runtime fresh scores use graph=null → 0; documented in PRD §15 |
| R7 | Pre-1.0 API churn (YouTube endpoints) | ETag caching; quota ledger; automatic RSS/oEmbed fallback |

---

## 12. Phase → Component Map

| Phase | Delivers |
|---|---|
| 0 | ✅ COMPLETE — Repo, config, M4 smoke gate (CRUD/WAL/backup round-trip), CLI skeleton, error taxonomy (engine later replaced by `tfdb`) |
| 1 | ✅ COMPLETE — Fetch (RSS/oEmbed/API+quota), Ingest, Storage, BM25 index, backup, CLI: ingest/score(basic)/backup/quota, rpc |
| 2 | ✅ COMPLETE — Scoring engine (SEO+GEO), ideas, keywords, scorecard, health, alerts, graph analytics |
| 3 | ✅ COMPLETE (Aug 4, 2026) — Thumbnail generator (HTML→image via chromiumoxide headless Chromium + `/assets` cleanup), `check availability`, `export` (CSV/ZIP), Filmot opt-in recovery, agent interface hardening |
| 4 | **IN PROGRESS (Aug 4, 2026)** — perf gate passed (5k videos < 30s on M4, release profile), release prep done (LICENSE files, Cargo.toml metadata, CHANGELOG, README, CI matrix), dashboard delivered; release pending repo push + tag v0.1.0 |
| 6 | ✅ COMPLETE (Aug 14, 2026) — Engine independence: `tfdb` replaces Turso/SQLite; own BM25 replaces tantivy; raw-Hyper replaces Axum; SSE replaces htmx; WebSocket JSON-RPC; content/`analyze` layer; growth forecasting; packaging-psychology; HNSW module (unwired); KG fully integrated |

---

## 13. ADRs (decision records — full context in session research)

| ADR | Decision | Alternatives rejected |
|---|---|---|
| ADR-1 | **Embedded `tfdb` engine** (from-scratch, `.wal`+`.dat`) | Turso/SQLite (external engine, v3 stack); SurrealDB (BUSL, scope); rusqlite-only (external, no Rust-native vision) |
| ADR-2 | **BM25 via TubeForge's own engine** (`src/search`) | tantivy (external, v3 stack); Turso FTS (beta ranking bugs) |
| ADR-3 | **Vector = HNSW module (ships, unwired)** | Turso vector ANN (roadmap only); sqlite-vec (pre-v1 alpha) |
| ADR-4 | **Graph = Rust adjacency + PageRank + Louvain** | SQL recursive CTEs (unsupported); GNN (no requirement) |
| ADR-5 | **Crash-safe WAL + atomic checkpoint** | MVCC-style multi-writer (out of scope; single-writer) |
| ADR-6 | **CLI-only v1 + local dashboard `serve`** | Multi-tenant web app (no auth surface; CSRF origin guard is the only cross-origin protection) |
| ADR-7 | **Backup before every batch ingest** (snapshot copy + integrity re-open) | Trusting engine durability |
| ADR-8 | **Stdio JSON-RPC agent bridge** (`tubeforge rpc`) — same method surface as `/ws`, over stdin/stdout | External `tursodb --mcp` (removed with storage); in-process MCP server |
| ADR-9 | **No embeddings in v1** (lexical BM25 + token overlap only) | Embedding pipelines (scope; revisit post-v1) |
| ADR-10 | **WebSocket JSON-RPC + SSE dashboard** (v1.3) | Axum + htmx polling (v3 stack); JS chart libraries |

---

## 14. Open Questions (HLD level)

1. SEO/GEO scoring spec (§5.2 of PRD) — signal weights/formulas authored and baked into defaults; per-component env overrides.
2. ~~Thumbnail HTML→image method (Phase 3): SVG+resvg vs headless Chromium.~~ → **Resolved (Aug 4, 2026):** headless Chromium via **chromiumoxide 0.9.1** (CDP) with chromiumoxide_fetcher-pinned Chromium; Tailwind v4 via standalone CLI; Blink determinism, pinned browser (no system Chrome), MIT/Apache-2.0.
3. ~~Exact engine + tantivy version pins~~ → **Resolved at Phase 0 gate** for the v3 stack; **superseded by v4.0 engine-independence (ADR-1/2).**
4. ANN vector wiring timing (post-release; embeddings pipeline deferred).
5. Windows support timing (v1 macOS-first).
