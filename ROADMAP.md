# TubeForge Product Roadmap & Engineering Milestones (v4.5 — Svelte 5 Architecture)

**Version** — 4.5
**Architecture** — Pure Rust Backend (`tfdb` + Hyper + WS/SSE) + Svelte 5 Frontend (Runes + Vite + Tailwind v4)
**Target Release** — v0.3.0 (Real-Time Creator Cockpit)
**Last Updated** — September 2, 2026

---

## 1. Roadmap Overview & Pipeline Flow

```
[PIPELINE FLOW]
Phase 0–6 Native Rust Engine (tfdb + BM25 + Hyper + WS + Louvain + Greedy) 
  ➔ Phase 7.1 Svelte 5 SPA Architecture & Real-Time RPC Client
  ➔ Phase 7.2 Visual SERP Grid & Media Cards with Outlier Multipliers
  ➔ Phase 7.3 Verifiable Evidence Ledger & Algorithmic Attribution
  ➔ Phase 7.4 In-Browser Interactive Kanban Board
  ➔ Phase 7.5 Script Studio & Native 60fps Teleprompter
  ➔ Phase 7.6 Live 1280x720 Thumbnail Preview Studio
  ➔ Phase 7.7 Real-Time WebSocket & SSE Event Ticker
```

---

## 2. Phase 7 Breakdown — Svelte 5 Real-Time Creator Cockpit

### Phase 7.1 — Svelte 5 SPA Foundation & High-Throughput RPC Client
* **Stack**: Svelte 5 (Runes `$state`, `$derived`, `$effect`) + Vite + Tailwind CSS v4 + Lucide Svelte.
* **Deliverables**:
  1. Initialize Svelte 5 frontend in `frontend/` with zero Virtual DOM runtime overhead.
  2. High-throughput WebSocket JSON-RPC 2.0 and SSE client stores using Svelte Runes.
  3. Single-binary embedding into Rust release binary via `include_dir!` (<50 KB gzipped total bundle).
* **Verification Gate**:
  * Frontend builds cleanly with `bun run build` / `npm run build` under 50 KB gzip.
  * WebSocket JSON-RPC duplex communication verified over loopback.

### Phase 7.2 — Visual SERP Grid & Media Cards
* **Goal**: Deliver rich, responsive video cards with 16:9 thumbnail previews.
* **Deliverables**:
  1. `MediaCard.svelte` with high-resolution thumbnail rendering and channel avatar badges.
  2. Outlier performance badges (e.g. `8.21x Multiplier`) computed against channel mean baselines.
  3. Real-time view velocity, publication distribution, and duration bars.
* **Verification Gate**:
  * Topic research renders visual thumbnail cards with zero layout jitter.

### Phase 7.3 — Verifiable Evidence Ledger & Attribution
* **Goal**: Provide transparent source citations for all 18 SEO and 7 GEO algorithmic signals.
* **Deliverables**:
  1. `EvidenceLedger.svelte` component.
  2. Collapsible citation cards linking title/description recommendations to exact BM25 documents and competitor tags.
  3. Clear visual separation between observed facts, statistical inferences, and private channel metrics.
* **Verification Gate**:
  * Every score signal renders expandable source citations with exact video IDs and term frequencies.

### Phase 7.4 — In-Browser Interactive Production Kanban Board
* **Goal**: Surface TubeForge's backend Kanban engine into an interactive drag-and-drop board.
* **Deliverables**:
  1. `KanbanBoard.svelte` with reactive columns (`todo`, `inprogress`, `done`, `published`).
  2. One-click ticket creation from topic research and competitor gap analysis.
  3. Direct generation of 0:00–0:45 First-Screen retention prompt contracts.
  4. Instant state persistence to `tfdb` via WebSocket JSON-RPC (`kanban.*`).
* **Verification Gate**:
  * Dragging a ticket between columns persists immediately to `tfdb` and updates connected clients.

### Phase 7.5 — Script Studio & Native 60fps Teleprompter
* **Goal**: Enable creators to draft, structure, and record retention-optimized scripts.
* **Deliverables**:
  1. `Teleprompter.svelte` with full-screen focus mode and native 60fps easing via `svelte/motion`.
  2. Dynamic WPM scroll speed controls and spacebar play/pause shortcuts.
  3. Cue markers, section timers, and elapsed time HUD.
* **Verification Gate**:
  * Teleprompter scrolls smoothly without dropping frames across 100–250 WPM reading speeds.

### Phase 7.6 — Live 1280x720 Thumbnail Preview Studio
* **Goal**: Interactive visual editor for deterministic Chromium HTML/CSS thumbnail generation.
* **Deliverables**:
  1. `ThumbnailStudio.svelte` with 16:9 live preview canvas.
  2. Template customization (badge text, typography size, high-contrast background themes, accent gradients).
  3. Single-click trigger for headless Chromium render and PNG download.
* **Verification Gate**:
  * Canvas preview renders deterministically matching `tubeforge thumbnail render` output.

### Phase 7.7 — Real-Time WebSocket & SSE Event Ticker
* **Goal**: Transform the dashboard into an active, living monitoring cockpit.
* **Deliverables**:
  1. Live ticker widget in `Dashboard.svelte` connected to `/events` and `/ws`.
  2. Real-time logging of background `greedy daemon` topic hunting and SERP rank changes.
* **Verification Gate**:
  * Background topic hunter executions and ingestion batches stream live to the UI without page refresh.

---

## 3. Implementation Schedule

| Milestone | Task / Deliverable | Status |
|---|---|---|
| M7.1 | Svelte 5 Scaffold & Runes RPC Client | Ready to Execute |
| M7.2 | Visual SERP Grid (`MediaCard.svelte` & `TopicResearch.svelte`) | Ready to Execute |
| M7.3 | Evidence Ledger Component (`EvidenceLedger.svelte`) | Ready to Execute |
| M7.4 | Interactive Kanban Board (`KanbanBoard.svelte`) | Ready to Execute |
| M7.5 | Script Studio & Teleprompter (`Teleprompter.svelte`) | Ready to Execute |
| M7.6 | Live Thumbnail Studio (`ThumbnailStudio.svelte`) | Ready to Execute |
| M7.7 | Live WebSocket/SSE Event Ticker (`Dashboard.svelte`) | Ready to Execute |
| M7.8 | 3-Witness Full Stack Verification | Ready to Execute |
