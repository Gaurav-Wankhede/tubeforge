# TubeForge

**Local-First YouTube SEO/GEO Growth Engine & Analytics Intelligence**

[![CI Status](https://img.shields.io/badge/CI-Passing-brightgreen?style=flat-square&logo=githubactions)](https://github.com/Gaurav-Wankhede/tubeforge/actions/workflows/ci.yml)
[![Rust Version](https://img.shields.io/badge/Rust-1.85%2B-orange?style=flat-square&logo=rust)](https://github.com/Gaurav-Wankhede/tubeforge)
[![License](https://img.shields.io/badge/License-MIT%2FApache--2.0-blue?style=flat-square)](LICENSE-MIT)
[![Architecture](https://img.shields.io/badge/Architecture-Local--First%20%7C%20Agent--Native-purple?style=flat-square)](https://github.com/Gaurav-Wankhede/tubeforge)
[![Platform Support](https://img.shields.io/badge/Platform-macOS%20%7C%20Linux%20%7C%20Windows-lightgrey?style=flat-square)](https://github.com/Gaurav-Wankhede/tubeforge)

---

## Overview

TubeForge is a single-binary YouTube SEO/GEO growth engine and analytical platform written in pure Rust. It ingests public channel metadata (via RSS, oEmbed, keyless SERP research, and optional YouTube Data API v3), persists data into an embedded, crash-safe storage engine (`tfdb` with `.wal` + `.dat`), ranks titles and descriptions with an internal BM25 search engine, and autonomously discovers high-opportunity content topics.

TubeForge is designed to operate locally without cloud dependencies, external databases, SaaS subscriptions, or third-party web scrapers.

```
+-----------------------------------------------------------------------------------------+
|                                       TUBEFORGE                                         |
+--------------------------+-------------------------------+------------------------------+
|      DATA INGESTION      |       ANALYTICS & ENGINE      |          INTERFACES          |
|  * Keyless RSS & oEmbed  |  * tfdb (Embedded WAL + DAT)  |  * Stdio JSON-RPC (Agents)   |
|  * Keyless SERP / yt-dlp |  * BM25 Multi-Index Engine    |  * Hyper + SSE + WS Web UI   |
|  * Optional Data API v3  |  * 18 SEO + 7 GEO Scorers     |  * CLI with JSON Envelopes   |
|  * Transcript Heatmaps   |  * Louvain Topic Clustering   |  * Headless Chromium Thumbs  |
+--------------------------+-------------------------------+------------------------------+
```

---

## Core Engineering Features

- **Local-First & Private**: Zero external database servers, SaaS scrapers, or third-party analytics trackers. All channel intelligence, keyword indices, and draft metadata remain on your local machine.
- **Embedded `tfdb` Storage Engine**: A crash-safe, write-ahead-log (`.wal` + `.dat`) storage system implemented in pure Rust without SQLite, PostgreSQL, or C runtime dependencies.
- **Mathematical SEO & GEO Scoring**: Computes an 18-component SEO score and a 7-component Generative Engine Optimization (GEO) score against real-time BM25 index corpus baselines.
- **Autonomous Greedy Engine**: Discovers, qualifies, and tracks high-demand, low-competition video topics on autopilot using channel graph tags, competitor analytics, and search suggestion drift.
- **Embedded Web Dashboard**: Server-rendered UI on raw Hyper featuring Server-Sent Events (SSE) and WebSocket JSON-RPC channels with server-rendered inline SVGs and zero JavaScript framework dependencies.
- **Agent-Native Protocol (`tubeforge rpc`)**: Direct stdio JSON-RPC protocol interface for AI coding harnesses (OpenCode, Claude Code, Codex, Hermes, Pi Agent) to query channel intelligence on demand.
- **Deterministic Thumbnail Renderer**: Generates pixel-perfect 1280x720 PNG thumbnails from HTML/CSS templates via headless Chromium.

---

## Installation & Quickstart

### Prerequisites

- Rust 1.85 or later (`rustup update stable`)
- Git

### Build from Source

```sh
# Clone repository
git clone https://github.com/Gaurav-Wankhede/tubeforge.git
cd tubeforge

# Build release binary
cargo build --release

# Optional: Add to user path
ln -s "$(pwd)/target/release/tubeforge" /usr/local/bin/tubeforge
```

### Initial Setup & Workflow

```sh
# 1. Initialize data directory (~/.tubeforge) and scaffold configuration
tubeforge init

# 2. Ingest channel history (via RSS, ETag-cached, zero API key needed)
tubeforge ingest channels UC_x5XG1OV2P6uZZ5FSM9Ttw

# 3. Ingest video links from standard input (oEmbed)
printf 'https://www.youtube.com/watch?v=dQw4w9WgXcQ\n' | tubeforge ingest links

# 4. Score a draft title against 18 SEO + 7 GEO algorithmic signals
tubeforge score --draft-title "Rust Memory Management: How Ownership Actually Works"

# 5. Run keyless SERP demand and competition analysis on a topic
tubeforge analyze "distributed consensus in rust"

# 6. Generate data-driven topic suggestions based on channel graph
tubeforge ideas

# 7. Start the local monitoring dashboard
tubeforge serve
```

---

## Command Reference

TubeForge provides a unified CLI interface. Every command supports the `--json` flag to emit stable `{ok, data, meta, error}` envelopes for scripting and AI agents.

### Ingestion & Data Gathering

| Command | Description |
|---|---|
| `tubeforge ingest channels <id>...` | Ingests ~15 most recent videos per channel via RSS (ETag-aware). |
| `tubeforge ingest links` | Ingests video metadata from newline-separated URLs via oEmbed. |
| `tubeforge refresh` | Re-fetches all indexed channels; skips writes on HTTP 304 Not Modified. |
| `tubeforge transcript get <id>` | Extracts manual and auto-generated video captions via `yt-dlp`. |
| `tubeforge metadata <id>` | Ingests audience retention heatmaps and live engagement statistics. |
| `tubeforge comments get <id>` | Ingests audience comments for community feedback analysis. |

### Scoring & Topic Intelligence

| Command | Description |
|---|---|
| `tubeforge score --draft-title <t>` | Computes 18 SEO + 7 GEO composite scoring metrics with actionable recommendations. |
| `tubeforge analyze <topic>` | Real-time keyless SERP research assessing topic demand, competition, and packaging. |
| `tubeforge ideas` | Suggests high-affinity video concepts via Louvain topic clustering and PageRank. |
| `tubeforge gaps` | Mines content and tag opportunities across tracked competitor channels. |
| `tubeforge keywords track <kw>` | Tracks rank positions for target keywords across search queries. |
| `tubeforge tags analyze` | Analyzes channel tag coverage, clustering density, and missing keywords. |
| `tubeforge forecast` | Computes channel growth trajectories via Ordinary Least Squares (OLS). |

### Autonomous Topic Engine (`greedy`)

| Command | Description |
|---|---|
| `tubeforge greedy seeds init` | Seeds research queue from competitor tags, channel tags, and search trends. |
| `tubeforge greedy run [--max N]` | Autonomously researches top candidate topics from the discovery queue. |
| `tubeforge greedy daemon` | Runs the continuous topic-hunting engine in the background. |
| `tubeforge greedy status` | Displays execution history, cooldowns, and discovered opportunities. |
| `tubeforge greedy stop` | Gracefully terminates running background daemon via PID signal. |

### Interfaces & Operations

| Command | Description |
|---|---|
| `tubeforge serve [--port N]` | Starts loopback web dashboard with live SSE and WebSocket feeds. |
| `tubeforge rpc` | Stdio JSON-RPC bridge for autonomous AI agents and harnesses. |
| `tubeforge thumbnail render` | Renders 1280x720 PNG thumbnails from HTML/CSS templates via Chromium. |
| `tubeforge backup` | Creates a point-in-time snapshot of `tfdb` with integrity verification. |
| `tubeforge quota` | Displays YouTube Data API v3 daily usage ledger (resets midnight PT). |
| `tubeforge export --format zip\|dir` | Exports all tables, transcripts, and metrics to deterministic CSV/JSON. |
| `tubeforge reindex` | Rebuilds internal BM25 search indices across all stored video records. |

---

## Technical Architecture

### 1. Storage Engine (`tfdb`)
- **Write-Ahead Log (`.wal`)**: Append-only log ensuring transactional ACID durability and high write throughput.
- **Segmented Storage (`.dat`)**: Binary segments indexed in memory with CRC32 integrity checksums.
- **Recovery Protocol**: Automatic crash recovery with log replay during engine initialization.

### 2. Algorithmic Scoring Models
- **SEO Scorer (18 Components)**: Analyzes title length, keyword front-loading, search intent alignment, capitalization consistency, cognitive complexity, character entropy, and tag relevance.
- **GEO Scorer (7 Components)**: Evaluates content extractability by AI search models (Perplexity, ChatGPT, Google AI Overviews) focusing on entity salience, structured definitions, semantic clarity, and citation authority.

### 3. Agent JSON-RPC Interface (`tubeforge rpc`)
AI agents communicate directly over standard input/output using line-delimited JSON-RPC:

```json
-> {"id": "req-1", "method": "score.title", "params": {"title": "Async Rust in Practice"}}
<- {"id": "req-1", "type": "progress", "progress": 0.5, "message": "Evaluating BM25 corpus..."}
<- {"id": "req-1", "type": "result", "data": {"seo_score": 8.7, "geo_score": 8.2, "passed": true}}
```

---

## Configuration Reference

Configuration is managed via environment variables or a `.env` file in the project root:

| Variable | Default | Description |
|---|---|---|
| `TUBEFORGE_DATA_DIR` | `~/.tubeforge` | Root directory for application state and databases. |
| `TUBEFORGE_DB_PATH` | `~/.tubeforge/data` | Path to `tfdb` segment and write-ahead log files. |
| `TUBEFORGE_BACKUP_DIR` | `~/.tubeforge/backups` | Target directory for automated database snapshots. |
| `TUBEFORGE_BACKUP_KEEP` | `7` | Number of point-in-time snapshots to retain. |
| `TUBEFORGE_SERVE_PORT` | `8080` | Local port for the web dashboard. |
| `YOUTUBE_API_KEY` | *(None)* | Optional YouTube Data API v3 key (keyless mode active if unset). |
| `TUBEFORGE_WEIGHTS_SEO` | `1.0` | Global multiplier for standard SEO scoring. |
| `TUBEFORGE_WEIGHTS_GEO` | `1.0` | Global multiplier for Generative Engine Optimization scoring. |
| `LOG_LEVEL` | `info` | Logging verbosity (`trace`, `debug`, `info`, `warn`, `error`). |

---

## Quality Assurance & Verification

```sh
# Run full unit and integration test suites
cargo test

# Run compiler lints with warnings denied
cargo clippy --all-targets -- -D warnings

# Check code formatting conformance
cargo fmt --check
```

---

## License

TubeForge is dual-licensed under either:
- **MIT License** ([LICENSE-MIT](LICENSE-MIT))
- **Apache License, Version 2.0** ([LICENSE-APACHE](LICENSE-APACHE))

at your option.

