# Architecture

## System Overview

Sports-arb-h2 is an automated sports arbitrage detection and execution system. It continuously scans odds from three sportsbooks (DraftKings, FanDuel, Pinnacle), detects cross-venue arbitrage opportunities where guaranteed profit exists, and places both legs automatically via stealth browser sessions.

The system is split into three processes that communicate over WebSocket and HTTP:

```
                                    ┌─────────────────────────────────┐
                                    │  Rust Engine (port 3401)        │
                                    │                                 │
                                    │  Polls odds every 5s:           │
                                    │  - DK via dk-proxy (port 3403)  │
                                    │  - FD direct HTTP               │
                                    │  - PIN direct HTTP              │
                                    │                                 │
                                    │  In-memory order book           │
                                    │  Arb detection (sigma < 1.0)    │
                                    │  Safety: drift, circuit breaker │
                                    │  SQLite persistence             │
                                    │  REST API for settings          │
                                    └───────────┬─────────────────────┘
                                                │ WebSocket (port 3402)
                                                │ PlaceBetCommand / FillReport
                                                ▼
┌────────────────┐              ┌──────────────────────────────────────┐
│  DK Proxy      │              │  Bet Placer (port 3402)        │
│  (port 3403)   │              │                                      │
│  Node.js HTTP  │              │  3 Patchright Chrome windows:        │
│  proxy for DK  │              │  - DraftKings  (page.evaluate)       │
│  odds fetching │              │  - FanDuel     (context.request)     │
│  (TLS fix)     │              │  - Pinnacle    (context.request)     │
└────────────────┘              │                                      │
                                │  Receives bet commands via WS        │
                                │  Routes to correct adapter           │
                                │  Returns fill reports                │
                                └──────────────────────────────────────┘
```

## Components

### Rust Engine (`engine/`)

The brain of the system. Runs a Tokio async runtime with:

- **Odds Fetcher** (`odds_fetcher.rs`) — Concurrent HTTP fetchers for DK (via proxy), FD, and Pinnacle. Fetches 15+ endpoints in parallel every 5 seconds.
- **Order Book** (`models.rs`) — Two-level HashMap: `MarketIdentityKey -> VenueSelectionKey -> PriceEntry`. Groups the same bet across venues for comparison.
- **Arb Detector** (`arb_detector.rs`) — For each market, checks all cross-venue pairs. If `sigma = 1/odds_a + 1/odds_b < 1.0`, it's a guaranteed-profit arb. Draw selections are excluded from moneyline pairing (3-way soccer markets are not valid 2-way arbs).
- **Team Names** (`team_names.rs`) — 200+ team name normalizations across all sports. Also normalizes "Draw"/"Tie" selections to `"draw"` to support the arb detector's draw exclusion.
- **Stake Optimizer** (`stake_optimizer.rs`) — Proportional stake sizing to equalize payouts across both legs.
- **Live Executor** (`live_executor.rs`) — Sends bet commands over WebSocket to the bet placer. FD-first ordering (soft book). 20s fill timeout.
- **Paper Executor** (`paper_executor.rs`) — Simulated instant fills for testing.
- **Safety Systems**:
  - Odds drift verification — rejects if odds moved >3% since detection (`odds_verifier.rs`)
  - Circuit breaker — pauses trading on excessive losses or rejections (`circuit_breaker.rs`)
  - Rate limiter — per-venue sliding window bet limits (`rate_limiter.rs`)
  - Rescue hedger — computes hedge when leg 2 fails after leg 1 fills (`rescue_hedger.rs`)
- **SQLite DB** (`db.rs`) — Persists balances, trades, arb opportunities, settings.
- **REST API** (`api.rs`) — Axum handlers on port 3401 for settings, balances, trades, stats.
- **WebSocket Broadcaster** (`ws.rs`) — Pushes real-time events to any connected client.

### Bet Placer (`bet-placer/`)

The hands of the system. A Node.js process that:

1. Launches 3 Patchright (stealth Playwright) Chrome windows — one per sportsbook
2. Holds authenticated sessions (cookies, localStorage tokens)
3. Runs a WebSocket server on port 3402
4. Receives `PlaceBetCommand` from the engine, routes to the correct adapter, returns `FillReport`
5. Provides a CLI for manual operations (`balance`, `bet`, `info`, `status`)

### DK Proxy (`dk-proxy.js`)

A minimal Node.js HTTP proxy on port 3403. The Rust engine's `reqwest` library cannot complete TLS handshakes with DraftKings' CDN on Windows. The proxy uses Node's `fetch` to relay DK odds API requests.

## Sportsbook Adapters

### DraftKings (`draftkings.js`)

- **Auth**: HttpOnly browser cookies sent via `credentials: "include"` in `page.evaluate(fetch(...))`
- **Bet flow**: `getSelectionInfo()` -> `calculateBets()` (returns `token` + `calculationId`) -> `placeBet()` -> `waitForConfirmation()` (poll up to 15s)
- **Host**: `gaming-ca-on.draftkings.com/api/wager/v1`
- **Selection ID format**: `0ML{marketId}_{outcomeIndex}` (moneyline), `0HC{marketId}...` (spread), `0OU{marketId}...` (total)

### FanDuel (`fanduel.js`)

- **Auth**: JWT tokens from `localStorage["session-sportsbook"]` and `localStorage["location_token"]`, plus PerimeterX cookies via `context.cookies()`
- **Bet flow**: `getMarketPrices()` -> `implyBets()` (returns `betReference`) -> `placeBet()`
- **All requests via `context.request.post()`** (Node-side HTTP) — bypasses PerimeterX CORS blocking
- **Hosts**: `sib.on.sportsbook.fanduel.ca` (imply), `spb.on.sportsbook.fanduel.ca` (place), `smp.on.sportsbook.fanduel.ca` (prices)
- **Selection ID format**: `{marketId}:{selectionId}` (e.g., `801.161172464:60427`)

### Pinnacle (`pinnacle.js`)

- **Auth**: Three headers — `x-api-key` (static app key), `x-device-uuid`, `x-session` (from `localStorage["Main:User"]`)
- **Bet flow**: `quote()` (returns `marketId` + current `price`) -> `placeBet()` (returns `PENDING_ACCEPTANCE`) -> poll `GET /bets?requestIds=` until accepted (~7-8s)
- **Host**: `api.arcadia.pinnacle.com/0.1`
- **Selection ID format**: `{matchupId}:{marketKey}:{designation}` (e.g., `1627197304:s;0;m:home`)
- **No PerimeterX** — cleanest API of the three

## In-Memory Order Book

The book uses a two-level HashMap to group prices from different venues for the same bet:

```
MarketIdentityKey (event_id + market_type + line + period + is_live)
  └── VenueSelectionKey (venue + selection)
        └── PriceEntry (odds, timestamp, metadata)
```

Example:
```
NBA:2026-04-01:BOS-vs-MIA | moneyline | null | full_game | false
  ├── (draftkings, BOS) → 1.50, received 1711700001234
  ├── (fanduel, BOS)    → 1.48, received 1711700001456
  ├── (pinnacle, BOS)   → 1.55, received 1711700001100
  ├── (draftkings, MIA) → 2.64, received 1711700001234
  ├── (fanduel, MIA)    → 2.70, received 1711700001456
  └── (pinnacle, MIA)   → 2.60, received 1711700001100
```

Entries expire after 30 seconds (`MAX_DATA_AGE_MS`). The book uses diff-based updates — only entries where odds actually changed are written.

## WebSocket Protocol (Engine <-> Bet Placer)

**Engine -> Listener:**
```json
{"type":"place_bet","attempt_id":"uuid-L1","venue":"pinnacle","venue_market_id":"1627197304:s;0;m:home","selection":"LAC","stake":1.27,"expected_odds":1.95}
{"type":"get_balances"}
{"type":"ping"}
```

**Listener -> Engine:**
```json
{"type":"fill_report","attempt_id":"uuid-L1","status":"filled","accepted_stake":1.27,"accepted_odds":1.95,"error_message":null}
{"type":"balances","balances":[{"venue":"draftkings","balance":6.92},{"venue":"fanduel","balance":19.65},{"venue":"pinnacle","balance":3932.00}]}
{"type":"balance_update","venue":"pinnacle","balance":3930.73}
{"type":"pong"}
```

## File Structure

```
sports-arb-h2/
├── engine/                         <- Rust (Tokio + Axum)
│   ├── Cargo.toml
│   ├── .cargo/config.toml          <- MSVC linker paths
│   └── src/
│       ├── main.rs                 <- Entry point, polling loop
│       ├── config.rs               <- Sports/leagues, API hosts, constants
│       ├── models.rs               <- Data structures, order book
│       ├── odds_fetcher.rs         <- DK/FD/PIN odds fetchers
│       ├── team_names.rs           <- 200+ team name normalizations
│       ├── arb_detector.rs         <- Sigma calc, cross-venue detection
│       ├── stake_optimizer.rs      <- Proportional stake sizing
│       ├── live_executor.rs        <- WebSocket bet execution
│       ├── paper_executor.rs       <- Simulated execution
│       ├── odds_verifier.rs        <- Drift check (3% threshold)
│       ├── circuit_breaker.rs      <- Loss/rejection rate breaker
│       ├── rate_limiter.rs         <- Per-venue bet limits
│       ├── rescue_hedger.rs        <- Hedge computation (future)
│       ├── policy.rs               <- Multi-gate validation (future)
│       ├── state_machine.rs        <- Trade lifecycle FSM (future)
│       ├── treasury.rs             <- Capital tracking (future)
│       ├── trades.rs               <- Structured logging (future)
│       ├── db.rs                   <- SQLite persistence
│       ├── api.rs                  <- REST handlers
│       └── ws.rs                   <- WebSocket broadcaster
├── bet-placer/               <- Node.js + Patchright
│   ├── package.json
│   ├── index.js                    <- WS server, adapter dispatch, CLI
│   └── src/
│       ├── browser-manager.js      <- Patchright browser lifecycle
│       ├── arb-placer.js           <- Parallel bet coordinator
│       └── sportsbooks/
│           ├── index.js            <- URL registry
│           ├── draftkings.js       <- DK adapter
│           ├── fanduel.js          <- FD adapter
│           └── pinnacle.js         <- PIN adapter
├── dk-proxy.js                     <- DK TLS proxy (port 3403)
├── data/                           <- Runtime data (gitignored)
│   ├── profiles/                   <- Chromium user data per book
│   ├── cookies/                    <- Session snapshots
│   └── trades.db                   <- SQLite database
├── docs/                           <- Documentation
├── CLAUDE.md                       <- AI assistant context
└── session.md                      <- Project history
```
