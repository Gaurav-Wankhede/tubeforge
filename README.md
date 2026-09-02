```text
████████╗██╗   ██╗██████╗ ███████╗███████╗ ██████╗ ██████╗  ██████╗ ███████╗
╚══██╔══╝██║   ██║██╔══██╗██╔════╝██╔════╝██╔═══██╗██╔══██╗██╔════╝ ██╔════╝
   ██║   ██║   ██║██████╔╝█████╗  █████╗  ██║   ██║██████╔╝██║  ███╗█████╗  
   ██║   ██║   ██║██╔══██╗██╔══╝  ██╔══╝  ██║   ██║██╔══██╗██║   ██║██╔══╝  
   ██║   ╚██████╔╝██████╔╝███████╗██║     ╚██████╔╝██║  ██║╚██████╔╝███████╗
   ╚═╝    ╚═════╝ ╚═════╝ ╚══════╝╚═╝      ╚═════╝ ╚═╝  ╚═╝ ╚═════╝ ╚══════╝
```

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
- **Embedded `tfdb` Storage Engine**: A crash-safe, write-ahead-log (`.wal` + `.dat`) storage system implemented in pure Rust without SQLite, PostgreSQL, or C runtime dependencies, featuring atomic disk checkpointing and high-throughput batch writes.
- **High-Performance Knowledge Graph**: In-memory graph construction, Louvain community detection (579 communities in 42ms), and weighted PageRank (790M ops/sec) with atomic batch persistence and sub-4-second warm graph scoring.
- **Calibrated Mathematical SEO & GEO Scoring**: Computes an 18-component SEO score, a 7-component Generative Engine Optimization (GEO) score, and 3 Graph-Aware authority percentiles ($0\text{--}100$) against real-time BM25 index corpus baselines without artificial zero floors.
- **Live Keyword Ranking Engine**: Automated position tracking across own and competitor videos, recording real-time SERP ranks, deltas, and keyword opportunity scores.
- **Autonomous Greedy Engine**: Discovers, qualifies, and tracks high-demand, low-competition video topics on autopilot using channel graph tags, competitor analytics, and search suggestion drift.
- **Built-in Production Kanban Engine**: Native video production TODO and roadmap system interconnected directly with TubeForge's keyword research, SEO opportunity scores, and competitor SERP analytics across dual-channel taxonomy (`TECHVERSE` and `BOOKVERSE`).
- **Embedded Web Dashboard**: Server-rendered UI on raw Hyper featuring Server-Sent Events (SSE) and WebSocket JSON-RPC channels with server-rendered inline SVGs and zero JavaScript framework dependencies.
- **Agent-Native Protocol (`tubeforge rpc`)**: Direct stdio JSON-RPC protocol interface for AI coding harnesses (OpenCode, Claude Code, Codex, Hermes, Pi Agent) to query channel intelligence on demand.
- **Deterministic Thumbnail Renderer**: Generates pixel-perfect 1280x720 PNG thumbnails from HTML/CSS templates via headless Chromium.
- **Lazy Default-Evaluation Discipline**: Audited clean of eager-evaluation fallbacks (`clippy::or_fun_call`) — all `unwrap_or*` defaults on non-trivial values are lazy (`unwrap_or_else`), keeping clock reads, JSON construction, and string allocations off hot paths; recoverable data failures emit structured warnings instead of silently skewing analytics.

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
```

---

## Cross-Platform Compatibility & Installation

TubeForge is engineered and verified for cross-platform operation across **macOS**, **Linux**, and **Windows**. The storage engine (`tfdb`), BM25 scoring pipeline, and web interfaces contain zero platform-locked dependencies.

### macOS (Apple Silicon & Intel)

- **Target Architectures**: `aarch64-apple-darwin` (Apple Silicon M1/M2/M3/M4) and `x86_64-apple-darwin` (Intel).
- **Prerequisites**: Xcode Command Line Tools (`xcode-select --install`).
- **Path Installation**:
  ```sh
  # Add release binary to user path
  cp target/release/tubeforge /usr/local/bin/
  # or for Apple Silicon Homebrew path:
  cp target/release/tubeforge /opt/homebrew/bin/
  ```
- **Data Location**: Defaults to `~/.tubeforge` (resolved via `$HOME`).

### Linux (Ubuntu, Debian, Fedora, Arch Linux)

- **Target Architectures**: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, and `x86_64-unknown-linux-musl` (fully static).
- **Prerequisites**: Standard build essentials and OpenSSL (or native TLS):
  ```sh
  # Debian / Ubuntu
  sudo apt-get update && sudo apt-get install -y build-essential pkg-config libssl-dev

  # Fedora / RHEL
  sudo dnf install -y gcc pkg-config openssl-devel

  # Arch Linux
  sudo pacman -S base-devel openssl
  ```
- **Path Installation**:
  ```sh
  sudo cp target/release/tubeforge /usr/local/bin/
  ```
- **Data Location**: Defaults to `~/.tubeforge` (or `$XDG_DATA_HOME/tubeforge` if configured).
- **Headless Environments**: For thumbnail generation on headless servers, ensure Chromium is accessible or allow the automated fetcher to download a local sandbox build.

### Windows (Windows 10, 11 & Windows Server)

- **Target Architectures**: `x86_64-pc-windows-msvc` (Native) and `x86_64-pc-windows-gnu`.
- **Prerequisites**: Visual Studio C++ Build Tools or MSVC toolchain (`rustup default stable-x86_64-pc-windows-msvc`).
- **PowerShell Setup & Execution**:
  ```powershell
  # Build native Windows executable
  cargo build --release

  # Add to User PATH or copy to a directory in PATH
  Copy-Item .\target\release\tubeforge.exe "$HOME\.cargo\bin\"

  # Initialize TubeForge
  tubeforge.exe init

  # Ingest channels via PowerShell
  tubeforge.exe ingest channels UC_x5XG1OV2P6uZZ5FSM9Ttw

  # Score draft titles
  tubeforge.exe score --draft-title "Rust Memory Management: How Ownership Actually Works"

  # Start dashboard
  tubeforge.exe serve
  ```
- **Data Location**: Defaults to `%USERPROFILE%\.tubeforge` (e.g., `C:\Users\<User>\.tubeforge`).
- **Windows Subsystem for Linux (WSL2)**: TubeForge runs natively inside Ubuntu/Debian on WSL2 with full feature parity.

### Platform Architecture & Portability Invariants

- **Storage Portability**: `tfdb` `.wal` and `.dat` binary files are structured with fixed-width, little-endian encodings and CRC32 checksums, allowing database files to be seamlessly moved across macOS, Linux, and Windows without conversion.
- **Path Normalization**: Internal storage, indexers, and export routines utilize Rust's standard `PathBuf` abstractions, preventing forward/backward slash corruption across operating systems.
- **Loopback Socket Safety**: The dashboard binds `127.0.0.1` / `::1` uniformly across Unix sockets and Windows Winsock.

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
| `tubeforge transcript get <id>` | Extracts manual and auto-generated video captions via `yt-dlp`; `--engine whisper|auto` falls back to local Whisper ASR (`vectron-whisper` GGML, shared with Vectron, offline `source="whisper_local"`). |
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

### Video Production & Roadmap (`kanban`)

| Command | Description |
|---|---|
| `tubeforge kanban from-research <topic> --channel <ch>` | Creates a ticket directly mapped to existing keyword research & SEO metrics. |
| `tubeforge kanban create --title <t> --channel <ch>` | Manually creates a new video production Kanban ticket. |
| `tubeforge kanban list [--status <s>] [--channel <c>]` | Lists production tickets filtered by status (`todo`, `inprogress`, `done`, `published`) or channel. |
| `tubeforge kanban move <id> <status> [--url <yt>]` | Transitions ticket lifecycle status and attaches published video URLs. |
| `tubeforge kanban show <id>` | Displays full ticket metadata and interconnected live keyword research. |
| `tubeforge kanban prompt <id>` | Generates a 0:00–1:00 First-Screen retention contract production blueprint for the ticket. |
| `tubeforge kanban delete <id>` | Removes a ticket from the production database. |

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

### 0. Shared Whisper ASR — Vectron Interconnect

- **Vectron-Whisper Bridge:** `tubeforge transcript get --engine whisper|auto` shares Vectron's Rust `vectron-whisper` crate (`whisper-rs`/`whisper.cpp` GGML, `symphonia`→`rubato` 16kHz mono, `normalize()`+WER) via path dep `../vectron/crates/vectron-whisper`. When `yt-dlp` captions are disabled, TubeForge extracts bestaudio then transcribes offline with shared model cache `<data>/models/whisper/ggml-base.bin`; same engine powers Vectron `VEC-008` (`SCRIPT.md` vs `voice/0N.wav`). Zero API cost, zero cloud.

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

### 4. Full-Stack Application Server (`tubeforge serve`)
`tubeforge serve` launches a comprehensive, standalone full-stack server powering both browser frontends and headless backend consumers:

- **Embedded Database & Graph Engine**: Manages `tfdb` connection state, coordinates transactional writes, and lazy-loads the in-memory Knowledge Graph (`kg`) for instant community detection and PageRank queries.
- **Comprehensive REST API Layer (`/api/*`)**: Exposes over 20+ specialized JSON endpoints (`/api/scores`, `/api/ideas/analyze`, `/api/keywords/inspect`, `/api/gaps/outliers`, `/api/scorecard`, `/api/transcripts`, `/api/tags/gaps`) for complete programmatic control.
- **Real-Time Server-Sent Events (`/events`)**: Streams live push notifications for database insertions, health counters, and system alerts with automated keep-alive heartbeats.
- **Duplex WebSocket JSON-RPC (`/ws`)**: High-throughput bidirectional socket channel sharing the unified RPC method surface with the CLI.
- **Zero-Dependency Static Host**: Directly serves the optimized React SPA (from `frontend/dist`) or fallback server-rendered HTMX pages with inline SVGs, requiring no Node.js runtime, npm, or external CDNs in production.

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

# Verify no eager-evaluation default-value calls (enforced codebase invariant)
cargo clippy --all-targets -- -W clippy::or_fun_call

# Check code formatting conformance
cargo fmt --check
```

The codebase maintains a zero-tolerance policy for eager evaluation in `unwrap_or`/`or` fallback positions: any default more expensive than a constant must be wrapped in a lazy closure (`unwrap_or_else`). Fallback semantics are chosen fail-safe — e.g., corrupt timestamps read as *unknown freshness* (epoch), never as *current*.

---

## License

TubeForge is dual-licensed under either:
- **MIT License** ([LICENSE-MIT](LICENSE-MIT))
- **Apache License, Version 2.0** ([LICENSE-APACHE](LICENSE-APACHE))

at your option.

