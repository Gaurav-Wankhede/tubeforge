# TubeForge Browser Extension

VidIQ-style overlay for YouTube, powered by your **local** TubeForge server (keyless — no API key, no quota, no data leaves your machine).

## Features

| Surface | What you get |
|---|---|
| **Watch pages** (`youtube.com/watch`) | Draggable panel: SEO/Total/GEO score, performance signals (VPH, engagement, retention, trending), the video's tags, a **keyword-research box** (type a topic → keyword score, opportunity/competition, verdict, suggested tags), and a link to the dashboard |
| **Search results** (`youtube.com/results`) | `TF <score>` badge on video cards for videos already in your DB |
| **Channel pages** (`/@handle`) | Channel Audit score + grade badge |
| **Popup** | Server status + quick keyword research from any tab |

## Install (unpacked)

1. Make sure the server is running: `tubeforge serve --port 17487` (with `TUBEFORGE_YTDLP_ENABLED=true` in `.env` for keyword research).
2. Open Chrome/Edge → `chrome://extensions`.
3. Toggle **Developer mode** (top-right).
4. Click **Load unpacked** → select this folder (`extensions/tubeforge`).
5. Open any YouTube video — the TubeForge panel appears top-right (drag it anywhere).

## How it works

- The content script talks to the **background service worker** via messages.
- The worker fetches `http://127.0.0.1:17487/api/*` — `host_permissions` means extensions bypass CORS, so the loopback-only server needs **zero changes** and stays secure (no auth, no cookie exposure, nothing reachable from random websites).
- No YouTube data is ever uploaded anywhere.

## Troubleshooting

| Symptom | Fix |
|---|---|
| Panel says "offline" | Start `tubeforge serve --port 17487` and refresh YouTube |
| Research says "could not load" | Set `TUBEFORGE_YTDLP_ENABLED=true` in `~/.tubeforge/.env` and restart serve |
| No badges on search | Only videos already scored in your DB get badges — ingest/score them first |
| Panel hidden behind YouTube UI | Drag the header; it's `z-index`-maxed but some overlays sit higher |

## Build / release notes

- MV3, no build step — plain JS.
- Icon placeholders are solid-color PNGs; replace with real branding later.
- Reload the extension after editing files (`chrome://extensions` → refresh icon).
