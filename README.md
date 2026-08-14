# TubeForge

Local-first YouTube SEO/GEO growth engine — a single Rust binary that ingests
free YouTube data (RSS + oEmbed, optionally the free Data API v3 with your own
key, plus opt-in yt-dlp for transcripts/comments/research), stores it in an
embedded **`tfdb` engine** (a from-scratch, crash-safe store in pure Rust —
`.wal` + `.dat`, no SQLite/SQL, no external database), and ranks
titles/descriptions/tags with **TubeForge's own BM25 engine** (no external
index).

Zero scraping, zero paid services, zero network listeners (except the optional
loopback-only dashboard). All data stays on your machine.

**Status:** Phases 0–6 complete (engine, ingest, scoring, analytics, content
layer, thumbnails, export, Knowledge Graph, agent hardening). Phase 4:
hardening & release.

## Quickstart

```sh
# 1. Init: data root, .env scaffold, DB + migrations
tubeforge init

# 2. Ingest a channel (RSS; ~15 most-recent videos, ETag-cached)
tubeforge ingest channels UC_x5XG1OV2P6uZZ5FSM9Ttw

# 3. Ingest individual video links (oEmbed; no API key needed)
printf 'https://www.youtube.com/watch?v=dQw4w9WgXcQ\n' | tubeforge ingest links

# 4. Score a draft title (18 SEO + 7 GEO components + recommendations)
tubeforge score --draft-title "Your 40-60 char title here"

# 5. Snapshot backup + integrity re-open, and API quota state
tubeforge backup
tubeforge quota

# 6. Rebuild the BM25 index from stored videos (idempotent)
tubeforge reindex
```

Every batch ingest runs an automatic snapshot backup (with integrity re-open)
before writing (`--no-backup` disables). `refresh` re-fetches known channels
ETag-aware (304 → no writes, no snapshot). **All commands** support `--json`,
wrapping output in the stable `{ok,data,meta,error}` envelope (LLD §4.2) —
ready for agent/script consumption.

## Commands

| Command | What it does |
|---|---|
| `tubeforge init` | Data root, `.env` scaffold, DB + migrations |
| `tubeforge ingest channels <id>...` | RSS ingest, ~15 most-recent videos, ETag-cached |
| `tubeforge ingest links` | oEmbed ingest from newline-separated URLs on stdin |
| `tubeforge refresh` | Re-fetch known channels (304 → no writes) |
| `tubeforge score --draft-title <t>` | BM25 + SEO/GEO composite envelope for a draft title |
| `tubeforge ideas` | PageRank/Louvain-influenced video idea candidates |
| `tubeforge keywords ...` | Keyword rank tracking + keyless SERP research |
| `tubeforge tags backfill\|analyze` | Tag entity backfill + coverage/gap analysis |
| `tubeforge transcript get\|list\|clear` | yt-dlp caption extraction (auto/manual subs) |
| `tubeforge metadata` / `comments get` | yt-dlp heatmap/live stats / comment extraction |
| `tubeforge analyze <topic>` | Realtime keyless SERP research → demand/supply gap + draft packaging |
| `tubeforge forecast` / `suggest <topic>` | OLS growth forecast / next-video recommendations |
| `tubeforge gaps` / `outliers` | Content & tag gap mining |
| `tubeforge scorecard` / `health` | Channel performance scorecard / data freshness census |
| `tubeforge alerts` | Video-unavailable & stale-channel alerts |
| `tubeforge check availability` | Batched `videos.list` availability check |
| `tubeforge backup` / `quota` / `reindex` | Snapshot + retention; API quota ledger; BM25 rebuild |
| `tubeforge export --format zip\|dir` | Deterministic export: CSVs + JSON arrays + manifest |
| `tubeforge filmot get <id>` | Opt-in Filmot recovery lookup (needs `TUBEFORGE_FILMOT_KEY`) |
| `tubeforge thumbnail render\|list-templates` | 1280×720 PNG thumbnails via headless Chromium |
| `tubeforge serve [--port] [--host]` | Local dashboard (raw-Hyper + WebSocket JSON-RPC + SSE, see below) |
| `tubeforge rpc` | **Stdio JSON-RPC bridge for agent harnesses** (OpenCode, Claude Code, Codex, Hermes, Pi Agent — see below) |
| `tubeforge prompt` | Assemble an AI gap-mining prompt bundle from stored transcripts |

## Dashboard

`tubeforge serve` starts a local, server-rendered dashboard — the deferred
PRD §5.4 item. It is built on a **raw-Hyper web framework** (no Axum/web
framework dependency) and streams live via **Server-Sent Events** (SSE, no
polling) with a **WebSocket JSON-RPC** channel for rich analysis. Pages:
dashboard home (health-card grid + inline-SVG charts), scores (top-100 table
with row-expand showing all **18 SEO + 7 GEO** components + graph signals +
performance badges), ideas, keyword rank trends (with SVG sparklines), alerts,
competitor scorecard, health report, gaps, and the audit page. Charts are
server-rendered inline SVG generated in Rust — no JS chart library (PRD §11).
`htmx.min.js` + `sse.js` are vendored in `static/` — offline-first, no CDN.

```sh
tubeforge serve                      # http://127.0.0.1:8080
tubeforge serve --port 9000          # custom port (TUBEFORGE_SERVE_PORT env)
```

**Local-first contract** (please read before using):

- **Loopback only.** The server binds `127.0.0.1` by default and refuses
  non-loopback hosts (`--host localhost` / `::1` allowed). It is single-user
  with **no authentication** — do not expose it to a network.
- **CSRF guard.** Mutating endpoints reject requests whose `Origin`/`Referer`
  host does not match the bound address (403). Requests with neither header
  are accepted (local scripts/agents).
- **Single writer.** `tfdb` is single-writer by design. Running `serve`
  concurrently with writing commands (`ingest`, `refresh`, `score`, …) is
  unsupported; concurrent *readers* are fine.
- **stdout purity.** `serve` is long-running and never emits the JSON
  envelope (LLD §4.2): the listening line goes to stderr. Ctrl-C shuts down
  cleanly.

## Agents: stdio JSON-RPC (`tubeforge rpc`)

Agent harnesses (OpenCode, Claude Code, Codex, Hermes, Pi Agent, Harness CLI)
connect to TubeForge for **analysis** via JSON-RPC over **stdio** — no separate
server, no MCP:

```sh
tubeforge rpc                        # reads one request per stdin line
```

Feed it line-delimited JSON-RPC — the **same method surface as the dashboard
WebSocket** (`/ws`): `scores.list`, `scores.detail`, `ideas.analyze`,
`keywords.*`, `gaps.get`, `analysis.*`, `health.get`, `scorecard.get`, … Each
request streams `progress` then a `result` (or `error`) — one JSON object per
line on stdout:

```
→ {"id":"r1","method":"health.get","params":{}}
← {"id":"r1","type":"progress","progress":0.5,"message":"Running health checks..."}
← {"id":"r1","type":"result","data":{...}}
```

stdout carries **only** responses; all diagnostics go to stderr. The process
runs until stdin EOF. The tfdb database is the storage source; the frontend
dashboard provides visual analysis.

## Configuration (`.env`)

`YOUTUBE_API_KEY` is optional. Empty → RSS + oEmbed only. When set, `--api`
enriches ingest with batched `videos.list` (≤50 ids/call, 1 unit/call, 10k/day,
ledger reset at midnight PT; `tubeforge quota` shows usage). See `.env.example`
for the full key list:

- Paths/data: `TUBEFORGE_DB_PATH`, `TUBEFORGE_DATA_DIR`, `TUBEFORGE_BACKUP_DIR`,
  `TUBEFORGE_BACKUP_KEEP`, `TUBEFORGE_QUOTA_WARN_AT`, `TUBEFORGE_STALE_DAYS`
- Dashboard: `TUBEFORGE_SERVE_PORT` (default 8080; `serve --port` wins)
- Scoring: `TUBEFORGE_WEIGHTS_SEO`, `TUBEFORGE_WEIGHTS_GEO`, and per-component
  overrides (`TUBEFORGE_SEO_*`, `TUBEFORGE_GEO_*` — sums normalized at use time)
- Own channel / growth: `TUBEFORGE_OWN_CHANNEL`
- Thumbnails: `TUBEFORGE_CHROMIUM_DIR` (pinned Chromium install dir,
  auto-downloaded on first render by chromiumoxide_fetcher, reused after)
- Opt-in third-party: `TUBEFORGE_FILMOT_KEY` (Filmot `get` only),
  `TUBEFORGE_YTDLP_*` (yt-dlp path/client/runtime)
- Logging: `LOG_LEVEL` (`trace|debug|info|warn|error`)

## Cross-platform

- **macOS (arm64)** is the first-class target (M-series).
- **Linux (x86_64/arm64) and Windows** are CI-tested (`cargo build`, clippy,
  tests) on every push — see `.github/workflows/ci.yml`.
- `tfdb` and the BM25 engine are **pure Rust** — no C dependencies, no system
  database required, cross-platform by construction.
- `thumbnail render` needs a Chromium binary; the chromiumoxide fetcher
  downloads a pinned build into `TUBEFORGE_CHROMIUM_DIR` on first render
  (no download during CI unit tests — the render test is `#[ignore]`).

## Design documents

- `PRD.md` — product requirements (data source policy §5.10, ingestion §5.3)
- `HLD.md` — architecture (data flows §5, storage §7, interfaces §8)
- `LLD.md` — the implementation contract (module layout §2, schema §3, CLI §4,
  fetch §5, ingest §6, backup/migrations §9)

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE),
at your option. All dependencies are permissive (tokio MIT, hyper MIT,
clap MIT/Apache-2.0, chromiumoxide MIT/Apache-2.0, etc.).
