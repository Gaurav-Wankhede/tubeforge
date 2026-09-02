# AGENTS.md — TubeForge Engineering & Growth Engine Harness

```
[PIPELINE FLOW]
Topic Intent ➔ Founder Playbook Triage ➔ TubeForge Research (SERP + Gaps + BM25) ➔ Kanban Creation (Zero-Colon Rule) ➔ Prompt Contract ➔ Execute ➔ 3-Witness Verify
```

---

## 1. Core Engineering Boundaries & Content Laws

```
[SYSTEM LAWS]
├── SCOPE: Local-first YouTube SEO/GEO growth engine, keyword analytics, Kanban tracking, and Founder Playbook integration.
├── STRICT BAN ON COLONS (':') IN TITLES & METADATA (HARD LAW):
│   ├── (1) The orchestrator, CLI commands, and all subagents MUST NEVER use colons in video titles, Kanban ticket titles, thumbnail text, or metadata strings.
│   ├── (2) Colons cause filesystem encoding issues, break search parsing, and signal low-effort template formatting.
│   └── (3) Always use parenthetical hooks (e.g. "How Linux Runs Code (Inside Syscalls & Memory Isolation)"), em-dashes (' — '), or direct question/claim phrasing.
├── FOUNDER PLAYBOOK & CONTENT STRATEGY ENGINE LAW: Every video topic, keyword analysis, ticket creation, and title/description generation MUST strictly invoke and apply the relevant Founder Playbook frameworks (.agents/skills/):
│   ├── (1) Triage & Diagnosis: `diagnose` (Meta-router for ambiguous topics, growth bottlenecks, or niche pivots)
│   ├── (2) Audience Discovery: `mom-test` (Anchor fluff, validate real developer problems, avoid hypothetical interest) & `four-steps` (Customer Development & Market Category)
│   ├── (3) Positioning & Category: `obviously-awesome` (Differentiated value themes, category design, competitive alternatives), `crossing-the-chasm` (Pragmatist tech adoption), `blue-ocean-strategy` (ERRC grid, noncustomer tiers)
│   ├── (4) Offer Packaging & Retention: `100m-offers` (Grand Slam Offer value equation, risk reversal, guarantees), `money-models` (30-day client-financed payback)
│   ├── (5) Distribution & Leads: `traction` (Bullseye 19 channels), `100m-leads` (Core Four acquisition + Rule of 100), `linkedin-strategy` (B2B authority distribution)
│   ├── (6) Messaging & Psychology: `storybrand` (SB7 Customer=Hero, Brand=Guide), `made-to-stick` (SUCCESs core hooks), `influence` (Cialdini 7 compliance levers), `spin-selling` (Situation/Problem/Implication/Need-payoff)
│   └── (7) Repository Longevity: `repo-longevity` (30-sec README, bit-rot CI defense, anti-hostile governance, release cadence)
├── REAL DATA INTEGRITY LAW (ZERO MOCK DATA DIRECTIVE):
│   ├── (1) All metrics, keyword positions, competitor gaps, and Kanban tickets MUST derive from pure SQLite database queries and live YouTube data.
│   └── (2) Never inject mock arrays, placeholder scores, or fictional metrics into CLI output or reports.
└── 3-WITNESS VERIFICATION:
    ├── (1) Artifact exists (code, ticket, or report generated).
    ├── (2) Diff landed (`git diff` clean with zero unhandled errors).
    └── (3) Tests and checks pass (`cargo check` and `cargo test` with 0 warnings).
```

---

## 2. Founder Playbook Routing Matrix for Video Topics

| Domain | Skill / Route | TubeForge Trigger & Capability |
|---|---|---|
| Startup Diagnostic | `diagnose` | Entry point for topic viability and channel pivot triage |
| Customer Discovery | `mom-test` | Identifying real developer friction vs hypothetical interest |
| Category Positioning | `obviously-awesome` | Differentiated angle, anti-commodity titles, competitive alternatives |
| Category Creation | `blue-ocean-strategy` | ERRC grid to identify underserved developer search gaps |
| Value Packaging | `100m-offers` | Value Equation for video packaging (Dream Outcome / Time Delay) |
| Audience Acquisition | `100m-leads` | Core Four acquisition, lead magnets, and Rule of 100 cadence |
| Narrative Architecture | `storybrand` | SB7 framework: Viewer is Hero, Creator is Guide |
| High-Retention Hooks | `made-to-stick` | SUCCESs principles (Simple, Unexpected, Concrete, Credible, Emotional, Stories) |
| Psychological Triggers | `influence` | 7 compliance levers (Social Proof, Authority, Scarcity, Unity) |
| Repository Longevity | `repo-longevity` | 30-sec README, bit-rot CI defense, anti-hostile governance |

---

## 3. Title & Packaging Rules

1. **Zero Colons**: All titles use parenthetical hooks or em-dashes.
2. **First 45 Characters**: Must convey the complete high-curiosity hook on mobile screen viewports.
3. **No Fluff / No Clickbait**: All titles must be backed by verifiable code and architectural derivations.
