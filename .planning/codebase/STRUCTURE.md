# Codebase Structure

**Analysis Date:** 2026-04-09

## Directory Layout

```
sportsbook-arbitrage/           # Project root
├── engine/                     # Rust scanning + detection engine (port 3401)
│   ├── src/                    # All Rust source files (flat, no subdirs)
│   ├── data/                   # Runtime data (SQLite DB)
│   ├── Cargo.toml              # Rust dependencies
│   ├── Cargo.lock
│   └── config.toml             # (Cargo config, not app config)
├── bet-placer/                 # Node.js bet execution server (port 3402)
│   ├── index.js                # WS server entry point + CLI
│   ├── src/
│   │   ├── browser-manager.js  # Patchright browser lifecycle
│   │   ├── arb-placer.js       # (utility, not the main entry)
│   │   └── sportsbooks/        # Per-venue adapters
│   │       ├── draftkings.js
│   │       ├── fanduel.js
│   │       ├── pinnacle.js
│   │       └── index.js        # SPORTSBOOKS registry (URLs etc.)
│   ├── package.json
│   └── node_modules/
├── data/                       # Shared browser session data
│   ├── profiles/               # Persistent Chromium user-data-dirs (one per venue)
│   │   ├── draftkings/
│   │   ├── fanduel/
│   │   └── pinnacle/
│   └── cookies/                # Saved cookie JSON files
├── docs/                       # Human-readable documentation
│   ├── architecture.md
│   ├── api-reference.md
│   ├── execution-flow.md
│   ├── setup.md
│   └── troubleshooting.md
├── .planning/                  # GSD planning artifacts
│   └── codebase/               # Codebase analysis documents
├── dk-proxy.js                 # DK TLS proxy (port 3403) — run from root
├── CLAUDE.md                   # Project instructions for Claude
└── session.md                  # Session handoff notes
```

## Directory Purposes

**`engine/src/`:**
- Purpose: All Rust source; flat structure, no subdirectories
- Contains: One `.rs` file per concern; `main.rs` is the monolithic entry point and polling loop orchestrator
- Key files:
  - `main.rs` — entry point, polling loop, safety chain orchestration
  - `models.rs` — all shared data types (`CanonicalMarket`, `Book`, `ArbCandidate`, `PaperTrade`, `WsEvent`, etc.)
  - `odds_fetcher.rs` — DK/FD/PIN fetchers and normalization
  - `arb_detector.rs` — sigma calculation and candidate detection
  - `live_executor.rs` — WebSocket-based real bet execution
  - `paper_executor.rs` — simulated execution for paper mode
  - `stake_optimizer.rs` — proportional stake math
  - `config.rs` — sport IDs, API hostnames, poll interval, data age
  - `team_names.rs` — canonical team name lookup map
  - `db.rs` — SQLite CRUD via rusqlite
  - `api.rs` — Axum HTTP handlers
  - `ws.rs` — WebSocket broadcaster
  - `circuit_breaker.rs` — loss + rejection rate breaker
  - `rate_limiter.rs` — per-venue hourly sliding window
  - `odds_verifier.rs` — drift check before execution
  - `rescue_hedger.rs` — rescue hedge math (not wired to execution)
  - `state_machine.rs` — `ArbState` enum (defined but not wired to live execution)
  - `policy.rs`, `treasury.rs`, `trades.rs` — additional modules

**`engine/data/`:**
- Purpose: Runtime-generated SQLite database
- Key files: `trades.db` — book_balances, paper_trades, arb_opportunities, settings tables
- Generated: Yes
- Committed: No

**`bet-placer/src/sportsbooks/`:**
- Purpose: One adapter class per venue
- Contains: `DraftKingsAdapter`, `FanDuelAdapter`, `PinnacleAdapter` — each implements `executeBet(selectionId, stake)`, `getBalance()`, `getSelectionInfo()`
- Key files: `draftkings.js`, `fanduel.js`, `pinnacle.js`, `index.js` (venue URL registry)

**`data/profiles/`:**
- Purpose: Persistent Chromium user-data-dirs; one per venue so sessions are isolated
- Generated: Yes (on first browser launch)
- Committed: No (gitignored — contains session cookies and auth state)

**`data/cookies/`:**
- Purpose: Manually saved cookie snapshots written by `adapter.saveCookies()`
- Generated: Yes
- Committed: No

**`docs/`:**
- Purpose: Human-readable reference docs; not loaded by the engine
- Key files: `execution-flow.md`, `api-reference.md`, `setup.md`, `troubleshooting.md`

## Key File Locations

**Entry Points:**
- `engine/src/main.rs` — Rust engine start
- `bet-placer/index.js` — Node.js bet placer start
- `dk-proxy.js` — DK proxy start (run from project root)

**Configuration:**
- `engine/src/config.rs` — Sport/league IDs, API hostnames, poll interval (5s), data age (30s), venues list
- `engine/Cargo.toml` — Rust dependencies and crate metadata
- `bet-placer/package.json` — Node.js dependencies

**Core Logic:**
- `engine/src/main.rs` — Polling loop, safety chain, mode branching (live/paper)
- `engine/src/arb_detector.rs` — Sigma detection, complementarity rules, safety pre-filters
- `engine/src/live_executor.rs` — Full execution sequence with two-stage sigma revalidation
- `engine/src/stake_optimizer.rs` — Stake math with balance-aware scaling
- `bet-placer/src/sportsbooks/draftkings.js` — DK `page.evaluate(fetch())` pattern
- `bet-placer/src/sportsbooks/fanduel.js` — FD `context.request.post()` pattern
- `bet-placer/src/sportsbooks/pinnacle.js` — PIN localStorage token extraction + direct fetch

**Data Models:**
- `engine/src/models.rs` — All shared structs and enums; single source of truth for data shapes

**Persistence:**
- `engine/src/db.rs` — All SQLite access; database file at `engine/data/trades.db`

**External Surface:**
- `engine/src/api.rs` — REST handlers (balances, trades, arbs, stats, settings, reset)
- `engine/src/ws.rs` — WebSocket broadcaster (broadcast channel, `handle_ws`)

## Naming Conventions

**Files:**
- Rust: `snake_case.rs` — one module per concern (`arb_detector.rs`, `stake_optimizer.rs`)
- Node.js: `kebab-case.js` for utilities (`browser-manager.js`, `arb-placer.js`), `lowercase.js` for adapters (`draftkings.js`, `fanduel.js`)

**Rust modules:**
- `pub fn` for cross-module API; `fn` for internal helpers
- Module names match filenames exactly (declared in `main.rs` with `mod`)

**Event IDs:**
- Format: `SPORT:YYYY-MM-DD:HOME-vs-AWAY` (e.g., `NBA:2026-03-15:LAL-vs-BOS`)
- Sport prefix is uppercase and matches `config::sports()` name field

**Selection names:**
- Canonical team codes: 2-3 letter uppercase (`LAL`, `BOS`, `KC`)
- Market sides: `over`, `under`, `draw` (lowercase)
- Resolved via `engine/src/team_names.rs`

**Venue IDs (selection format):**
- DK: `0ML{marketId}_{outcomeIndex}` (moneyline), `0HC{marketId}...` (spread), `0OU{marketId}...` (total)
- FD: `{marketId}:{selectionId}`
- PIN: `{matchupId}:{marketKey}:{designation}`

**Market types:**
- `moneyline`, `spread`, `total`, `team_total`, `player_prop`, `alt_spread`, `alt_total`

**Periods:**
- `full_game`, `first_half`, `second_half`, `first_quarter`, `second_quarter`, `third_quarter`, `fourth_quarter`, `first_period`, `second_period`, `third_period`, `regulation`

## Where to Add New Code

**New sport/league:**
- Add `SportConfig` entry to `engine/src/config.rs::sports()` with `dk_id`, `fd_page`/`fd_event_type`, `pin_league_ids`
- If soccer moneyline, add sport name to `THREE_WAY_SPORTS` in `engine/src/arb_detector.rs`
- Add team name mappings to `engine/src/team_names.rs`

**New safety filter:**
- Add check in `engine/src/main.rs` in the candidate loop after existing checks (after dedup, before execution)
- Or add a new module in `engine/src/` and declare with `mod` in `main.rs`

**New venue adapter:**
- Create `bet-placer/src/sportsbooks/{venue}.js` with class implementing `executeBet(selectionId, stake)`, `getBalance()`, `getSelectionInfo(selectionId)`
- Register in `bet-placer/src/sportsbooks/index.js`
- Add to adapter map in `bet-placer/index.js` (`main()`)
- Add venue name to `VENUES` in `engine/src/config.rs`
- Add venue to `BETTABLE_VENUES` in `engine/src/live_executor.rs`
- Seed initial balance in `engine/src/db.rs::seed_defaults()`

**New REST endpoint:**
- Add handler function in `engine/src/api.rs`
- Register route in `main.rs` Axum router

**New data model:**
- Add struct to `engine/src/models.rs`
- If persisted, add table DDL to `engine/src/db.rs::migrate()` and CRUD methods

**New WsEvent type:**
- Add variant to `WsEvent` enum in `engine/src/models.rs`
- Broadcast via `ws_tx.send(...)` in `main.rs` or relevant executor

**Utilities:**
- Shared math/helpers: new file in `engine/src/` declared with `mod` in `main.rs`
- Node.js utilities: `bet-placer/src/` (non-sportsbook helpers)

## Special Directories

**`data/profiles/`:**
- Purpose: Chromium persistent context user-data-dirs (one per venue: `draftkings/`, `fanduel/`, `pinnacle/`)
- Generated: Yes, on first `bet-placer/index.js` launch
- Committed: No — these contain real session auth state

**`data/cookies/`:**
- Purpose: JSON cookie snapshots written on clean exit by `adapter.saveCookies()`
- Generated: Yes
- Committed: No

**`engine/data/`:**
- Purpose: SQLite database `trades.db`
- Generated: Yes, on first engine run
- Committed: No

**`.planning/codebase/`:**
- Purpose: GSD codebase analysis documents consumed by `/gsd-plan-phase` and `/gsd-execute-phase`
- Generated: Yes, by `/gsd-map-codebase`
- Committed: Yes

---

*Structure analysis: 2026-04-09*
