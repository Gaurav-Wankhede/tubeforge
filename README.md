# TubeForge

Local-first YouTube SEO/GEO growth engine — a single Rust binary that ingests
free YouTube data (RSS + oEmbed, optionally the free Data API v3 with your own
key), stores it in an embedded [Turso Database](https://github.com/tursodatabase/turso)
(single-file SQLite-compatible `.db`, WAL mode), and ranks titles/descriptions
against a tantivy BM25 index.

Zero scraping, zero paid services, zero network listeners. All data stays on
your machine.

**Status:** Phases 0–3 complete (engine, ingest, scoring, analytics,
thumbnails, export, agent hardening). Phase 4: hardening & release.

## Quickstart

```sh
# 1. Init: data root, .env scaffold, DB + migrations
tubeforge init

# 2. Ingest a channel (RSS; ~15 most-recent videos, ETag-cached)
tubeforge ingest channels UC_x5XG1OV2P6uZZ5FSM9Ttw

# 3. Ingest individual video links (oEmbed; no API key needed)
printf 'https://www.youtube.com/watch?v=dQw4w9WgXcQ\n' | tubeforge ingest links

# 4. Basic score envelope (BM25 components + title length heuristic)
tubeforge score --draft-title "Your 40-60 char title here"

# 5. Backup (VACUUM INTO + integrity_check + retention) and quota state
tubeforge backup
tubeforge quota

# 6. Rebuild the tantivy index from the videos table (idempotent)
tubeforge reindex
```

Every batch ingest runs an automatic `VACUUM INTO` backup before writing
(`--no-backup` disables). `refresh` re-fetches known channels ETag-aware
(304 → no writes, no snapshot). **All commands** support `--json`, wrapping
output in the stable `{ok,data,meta,error}` envelope (LLD §4.2) — ready for
agent/script consumption.

## Commands

| Command | What it does |
|---|---|
| `tubeforge init` | Data root, `.env` scaffold, DB + migrations |
| `tubeforge ingest channels <id>...` | RSS ingest, ~15 most-recent videos, ETag-cached |
| `tubeforge ingest links` | oEmbed ingest from newline-separated URLs on stdin |
| `tubeforge refresh` | Re-fetch known channels (304 → no writes) |
| `tubeforge score --draft-title <t>` | BM25 + SEO/GEO composite envelope for a draft title |
| `tubeforge ideas` | PageRank-influenced video idea candidates |
| `tubeforge keywords` | Keyword rank tracking |
| `tubeforge scorecard` | Channel performance scorecard |
| `tubeforge health` | Data freshness/coverage census |
| `tubeforge alerts` | Video-unavailable & stale-channel alerts |
| `tubeforge check availability` | Batched `videos.list` availability check |
| `tubeforge backup` / `quota` / `reindex` | Snapshot + retention; API quota ledger; index rebuild |
| `tubeforge export --format zip\|dir` | Deterministic export: CSVs + JSON arrays + manifest |
| `tubeforge filmot get <id>` | Opt-in Filmot recovery lookup (needs `TUBEFORGE_FILMOT_KEY`) |
| `tubeforge thumbnail render\|list-templates` | 1280×720 PNG thumbnails via headless Chromium |
| `tubeforge serve [--port] [--host]` | Local HTMX dashboard (see below) |
| `tubeforge mcp` | Print `.mcp.json` snippet for agent integration |

## Dashboard (HTMX)

`tubeforge serve` starts a local, server-rendered dashboard — the deferred
PRD §5.4 item. Pages: dashboard home (health-card grid that auto-refreshes
every 30 s, latest alerts, top ideas, inline-SVG charts), scores (top-100
table with title filter and row-expand showing all 17 SEO/GEO components),
ideas (status marking via HTMX POST), keyword rank trends (with position
sparklines), alerts (mark-read / clear), competitor scorecard, and the full
health report. htmx 2.0.9 is vendored in `static/` and served locally —
offline-first, no CDN. Charts are server-rendered inline SVG generated in
Rust (no JavaScript chart library; PRD §11).

```sh
tubeforge serve                      # http://127.0.0.1:8080
tubeforge serve --port 9000          # custom port (TUBEFORGE_SERVE_PORT env)
```

**Local-first contract** (please read before using):

- **Loopback only.** The server binds `127.0.0.1` by default and refuses
  non-loopback hosts (`--host localhost` / `::1` allowed). It is single-user
  with **no authentication** — do not expose it to a network.
- **CSRF guard.** Mutating endpoints (idea status, alert read/clear) reject
  requests whose `Origin`/`Referer` host does not match the bound address
  (403). Requests with neither header are accepted (local scripts/agents).
- **Single writer.** The server opens one database connection for reads and
  reuses the CLI's write paths. Running `serve` concurrently with writing
  commands (`ingest`, `refresh`, `score`, `keywords check`, …) is
  unsupported; concurrent *readers* are fine (WAL).
- **stdout purity.** `serve` is long-running and never emits the JSON
  envelope (LLD §4.2): the listening line goes to stderr. Ctrl-C shuts down
  cleanly.

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
- Thumbnails: `TUBEFORGE_CHROMIUM_DIR` (pinned Chromium install dir,
  auto-downloaded on first render by chromiumoxide_fetcher, reused after)
- Opt-in third-party: `TUBEFORGE_FILMOT_KEY` (Filmot `get` only; TubeForge
  never embeds Filmot's key)
- Logging: `LOG_LEVEL` (`trace|debug|info|warn|error`)

## Cross-platform

- **macOS (arm64)** is the first-class target (M-series).
- **Linux (x86_64/arm64) and Windows** are CI-tested (`cargo build`, clippy,
  tests) on every push — see `.github/workflows/ci.yml`.
- Turso DB is cross-platform; the simsimd/aegis C dependencies compile on
  macOS without extra tooling.
- `thumbnail render` needs a Chromium binary; the chromiumoxide fetcher
  downloads a pinned build into `TUBEFORGE_CHROMIUM_DIR` on first render
  (no download during CI unit tests — the render test is `#[ignore]`).

## MCP (Claude Code / Cursor)

```sh
tubeforge mcp
```

prints a `.mcp.json`-compatible snippet pointing at
`tursodb <db> --mcp` (external MCP server, ADR-8). Install `tursodb` via
`curl -sSfL https://get.turso.tech/install.sh | sh`.

## Design documents

- `PRD.md` — product requirements (data source policy §5.10, ingestion §5.3)
- `HLD.md` — architecture (data flows §5, storage §7, interfaces §8)
- `LLD.md` — the implementation contract (module layout §2, schema §3, CLI §4,
  fetch §5, ingest §6, backup/migrations §9)

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE),
at your option. All dependencies are permissive (Turso MIT, tantivy MIT,
tokio MIT, clap MIT/Apache-2.0).
