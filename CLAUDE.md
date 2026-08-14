# TubeForge Project Instructions

## Server Run Command (IMPORTANT)

When the user says **"run the server"** (or any variant: "start the server", "run both servers",
"start it up", "restart the server"), run **BOTH** servers:

1. **Backend Rust server** — HTTP + WebSocket JSON-RPC backend.
   ```
   ./target/debug/tubeforge serve --host 127.0.0.1 --port 17487
   ```
   - Rebuild first if the binary is stale: `cargo build`
   - Verify it's listening on `127.0.0.1:17487`

2. **Frontend Vite dev server** — React SPA on `localhost:5173`.
   ```
   npm run dev   # run inside frontend/
   ```
   - Verify it's listening on `localhost:5173`
   - The Vite config proxies `/api` and `/ws` to the backend at `127.0.0.1:17487`.

### Procedure
1. Free the ports if in use (`pkill -f "target/debug/tubeforge serve"`, kill Vite).
2. Rebuild the backend binary (`cargo build`).
3. Start the backend server (nohup, log to `/tmp/tubeforge.out` / `/tmp/tubeforge.err`).
4. Start the Vite dev server (nohup, log to `/tmp/vite.log`).
5. Verify **both** are healthy:
   - Backend: `curl http://127.0.0.1:17487/api/healthz` → `{"ok":true}`
   - Frontend: `curl http://localhost:5173/` → HTML
   - WebSocket: a quick RPC probe on `ws://127.0.0.1:17487/ws`
6. Report both server PIDs + health status.

## Important: "Run the server" means BOTH servers, never just one.

## Testing: Property-Based Testing is MANDATORY (STRICT)

**ALWAYS use `proptest` over normal test cases.** This is a strict system requirement.

### Test Runner: Always use `cargo nextest` (NEVER `cargo test`)
- **ALWAYS** run tests via `cargo nextest run` (or `rtk nextest run`)
- **NEVER** use `cargo test` — nextest is faster, isolates tests, and is the project standard
- Use `rtk nextest run` for token-optimized output

### Property-Based Testing Rules
- **Every module** MUST have property-based tests as the primary test strategy.
- **Normal `#[test]` functions** are allowed ONLY for:
  - Trivially deterministic functions (e.g., pure string formatting)
  - Integration tests that require external fixtures
  - Tests where proptest is genuinely infeasible (document WHY)
- **ALL logic functions** MUST have `#[proptest]` covering:
  - Roundtrip properties (serialize/deserialize)
  - Invariants (e.g., PageRank mass conservation, entity ID uniqueness)
  - Edge cases (empty input, single element, maximum depth)
  - Composition properties (A → B → C == A → C)
- **Strategy**: Use `proptest!` with explicit strategies (`any::<T>()`, `prop_flatmap`, `prop_compose`)
- **Minimum**: At least 3 property tests per public function that has non-trivial logic
- **No excuses**: If you didn't write proptest, it's not done.

Example:
```rust
// WRONG — normal test only
#[test]
fn pagerank_center_dominates() {
    let kg = build_star_graph();
    let pr = pagerank(&kg);
    assert!(pr["center"] > pr["leaf"]);
}

// RIGHT — property-based test
#[proptest]
fn pagerank_mass_is_conserved(
    #[strategy(0..100)] node_count: usize,
    #[strategy(0..50)] edge_count: usize,
) {
    let kg = generate_random_graph(node_count, edge_count);
    let pr = pagerank(&kg);
    let sum: f64 = pr.values().sum();
    prop_assert!((sum - 1.0).abs() < 1e-9 || pr.is_empty());
}
```
