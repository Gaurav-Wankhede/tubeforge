# TubeForge Agent Instructions

## Role
TubeForge is a local-first YouTube SEO/GEO growth engine with two runtime processes that
ALWAYS run together. This agent is responsible for starting, verifying, and managing both.

## Non-Negotiable Rule: BOTH Servers, Never One

Any request to "run the server", "start the server", "start it up", "restart the server",
or "run both servers" means starting **BOTH** of these:

| # | Server | Command | Port | Logs |
|---|--------|---------|------|------|
| 1 | Backend (Rust: HTTP + WS JSON-RPC) | `./target/debug/tubeforge serve --host 127.0.0.1 --port 17487` | 17487 | `/tmp/tubeforge.out`, `/tmp/tubeforge.err` |
| 2 | Frontend (Vite React SPA) | `npm run dev` (in `frontend/`) | 5173 | `/tmp/vite.log` |

Running only one is a FAILURE. The frontend at `localhost:5173` proxies `/api` and `/ws`
to the backend at `127.0.0.1:17487`, so neither works without the other.

## Startup Sequence (exact order)
1. **Free ports** if occupied:
   - `pkill -f "target/debug/tubeforge serve"` (backend)
   - kill the Vite/node process on 5173
2. **Rebuild** the backend if stale: `cargo build` (from repo root)
3. **Start backend** (nohup, backgrounded, logs to /tmp):
   - `nohup ./target/debug/tubeforge serve --host 127.0.0.1 --port 17487 >/tmp/tubeforge.out 2>/tmp/tubeforge.err </dev/null &`
   - Wait ~5s, confirm `lsof -i :17487` shows LISTEN
4. **Start frontend** (nohup, backgrounded, logs to /tmp):
   - `nohup npm run dev >/tmp/vite.log 2>&1 </dev/null &` (from `frontend/`)
   - Wait ~5s, confirm `lsof -i :5173` shows LISTEN

## Verification Gate (HARD — do not report done without all three)
Run and confirm all pass:
1. **Backend HTTP**: `curl http://127.0.0.1:17487/api/healthz` → `{"ok":true}`
2. **Frontend**: `curl http://localhost:5173/` → returns HTML
3. **WebSocket RPC**: a real RPC probe on `ws://127.0.0.1:17487/ws`
   (e.g. Node `WebSocket` sending `{"id":"x","method":"scores.list","params":{}}` → receives a result)

Then report: **both PIDs** + health status of each.

## Notes
- First request to the backend after start has ~9s DB cold-start latency; subsequent are instant. Don't mistake this for a hang.
- `serve` binds loopback only (single-user). The DB is single-writer — do not run writing CLI commands concurrently.
- To stop: kill the backend PID and the Vite PID.
