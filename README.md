# TubeForge

Local-first YouTube SEO/GEO growth engine — a single Rust binary that ingests
free YouTube data (RSS + oEmbed, optionally the free Data API v3 with your own
key), stores it in an embedded [Turso Database](https://github.com/tursodatabase/turso)
(single-file SQLite-compatible `.db`, WAL mode), and ranks titles/descriptions
against a tantivy BM25 index.

Zero scraping, zero paid services, zero network listeners. All data stays on
your machine.

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
(304 → no writes, no snapshot). `--json` wraps all output in the stable
`{ok,data,meta,error}` envelope (LLD §4.2).

## Configuration (`.env`)

`YOUTUBE_API_KEY` is optional. Empty → RSS + oEmbed only. When set, `--api`
enriches ingest with batched `videos.list` (≤50 ids/call, 1 unit/call, 10k/day,
ledger reset at midnight PT; `tubeforge quota` shows usage). See `.env.example`
for all keys (`TUBEFORGE_DB_PATH`, `TUBEFORGE_DATA_DIR`, `TUBEFORGE_BACKUP_DIR`,
`TUBEFORGE_BACKUP_KEEP`, `TUBEFORGE_QUOTA_WARN_AT`, `LOG_LEVEL`).

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

MIT or Apache-2.0 (all dependencies permissive: Turso MIT, tantivy MIT,
tokio MIT, clap MIT/Apache-2.0).
