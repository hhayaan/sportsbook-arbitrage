# Arb Engine V5

Rust scanning, arbitrage detection, safety filtering, stake sizing, persistence, and API layer for the local sportsbook arbitrage bot.

V5 is a local multi-process system:

```text
Rust Engine (port 3401)
  - polls public odds APIs every 5 seconds
  - normalizes markets into one canonical shape
  - maintains an in-memory order book
  - detects cross-venue arbitrage
  - applies safety filters and stake sizing
  - persists balances, trades, settings, and arb logs to SQLite
  - exposes REST and WebSocket APIs

Bet Placer (Node.js, port 3402)
  - launches Patchright Chrome profiles for DraftKings, FanDuel, Pinnacle
  - keeps authenticated browser sessions alive
  - receives quote/place/balance commands over WebSocket
  - places live bets through sportsbook adapters

DK Proxy (Node.js, port 3403)
  - forwards DraftKings public odds API requests
  - works around Rust reqwest TLS issues with DraftKings on Windows
```

## Project Goal

The bot scans DraftKings, FanDuel, and Pinnacle odds across supported sports, identifies two-outcome arbitrage where:

```text
sigma = 1 / leg1_decimal_odds + 1 / leg2_decimal_odds
```

and treats `sigma < 1.0` as a candidate guaranteed-profit opportunity.

The engine is designed to:

1. Fetch and normalize odds from public sportsbook endpoints.
2. Keep a fresh in-memory book with 30-second expiry.
3. Detect valid cross-venue arbitrage candidates.
4. Block known fake arbs from market mismatches.
5. Size stakes against configured stake limits and current venue balances.
6. Execute trades in paper mode or live mode through the Node bet placer.
7. Persist operational history in SQLite.
8. Expose REST and WebSocket APIs for status, settings, balances, and trade history.

## Current Architecture

```text
                    +------------------------------+
                    | Rust Engine                  |
                    | engine/src/main.rs           |
                    |                              |
                    | HTTP API: 0.0.0.0:3401       |
                    | WS events: /ws               |
                    | SQLite: engine/data/trades.db|
                    +---------------+--------------+
                                    |
                                    | WebSocket commands
                                    v
                    +------------------------------+
                    | Node Bet Placer              |
                    | bet-placer/index.js          |
                    | WS server: localhost:3402    |
                    | Patchright Chrome profiles   |
                    +------------------------------+

                    +------------------------------+
                    | DK Proxy                     |
                    | dk-proxy.js                  |
                    | HTTP: localhost:3403         |
                    +------------------------------+
```

## Main Engine Modules

| Module | Purpose |
| --- | --- |
| `main.rs` | Entry point, settings/env handling, polling loop, safety orchestration, REST server |
| `config.rs` | Supported sports, league IDs, API hostnames, polling constants |
| `odds_fetcher.rs` | DraftKings, FanDuel, and Pinnacle odds fetchers and parsers |
| `team_names.rs` | Team, selection, event ID, and player name normalization |
| `models.rs` | Canonical market model, order book, arb/trade/settings/API event models |
| `arb_detector.rs` | Sigma detection, complementarity checks, Pinnacle requirement, market mismatch filters |
| `stake_optimizer.rs` | Balance-aware stake sizing and scaling |
| `paper_executor.rs` | Simulated execution and balance updates |
| `live_executor.rs` | WebSocket connection to bet placer, quote flow, live two-leg execution |
| `odds_verifier.rs` | Pre-execution odds drift checks |
| `circuit_breaker.rs` | Rolling loss and rejection breaker |
| `rate_limiter.rs` | Per-venue one-hour sliding bet window |
| `db.rs` | SQLite schema and CRUD |
| `api.rs` | REST handlers and `/ws` upgrade |
| `ws.rs` | Broadcast channel for realtime client events |

The following modules exist but are not wired into the current live execution path: `rescue_hedger.rs`, `treasury.rs`, `state_machine.rs`, `policy.rs`, and `trades.rs`.

## Running V5

### Prerequisites

- Windows with the MSVC Rust toolchain.
- Node.js.
- Chrome installed for Patchright.
- GeoComply PLC running for FanDuel.
- Logged-in DraftKings, FanDuel, and Pinnacle browser sessions for live mode.
- Ontario/CA sportsbook accounts and endpoints.

### Build

```powershell
cd C:\dev\sportsbook-arbitrage\engine
cargo build
```

### Paper Mode

Paper mode scans odds and simulates trades. The DK proxy should still be running so DraftKings odds can be fetched reliably on Windows.

```powershell
# Terminal 1
cd C:\dev\sportsbook-arbitrage
node dk-proxy.js

# Terminal 2
cd C:\dev\sportsbook-arbitrage\engine
cargo run
```

### Live Mode

Start the bet placer before the engine. The engine attempts its WebSocket connection only at startup.

```powershell
# Terminal 1
cd C:\dev\sportsbook-arbitrage
node dk-proxy.js

# Terminal 2
cd C:\dev\sportsbook-arbitrage\bet-placer
node index.js
```

Log in to DraftKings, FanDuel, and Pinnacle in the opened Chrome windows. In the bet placer terminal, run:

```text
balance
```

Then start the engine:

```powershell
# Terminal 3
cd C:\dev\sportsbook-arbitrage\engine
$env:LIVE_MODE="1"
$env:FIXED_STAKE="1"
$env:MAX_STAKE="2"
$env:MIN_ARB_PCT="0.5"
cargo run
```

## Runtime Settings

Most active settings come from SQLite and environment variables at startup. `engine/config.toml` is mostly legacy and is not the main runtime configuration source.

| Variable | Default | Description |
| --- | --- | --- |
| `LIVE_MODE` | `0` | `1` uses real bet placement through the bet placer; otherwise paper mode |
| `FIXED_STAKE` | `25` | Per-leg stake in fixed mode |
| `MAX_STAKE` | `50` | Max stake per leg |
| `MIN_ARB_PCT` | `0.5` | Minimum arb percent required for execution |
| `STAKE_MODE` | `fixed` | `fixed` or `proportional` |
| `PROPORTIONAL_PCT` | `5` | Percent of smaller venue balance used in proportional mode |
| `ENABLED` | `true` | `false` scans/logs but does not execute |
| `MAX_HOURS_TO_START` | `24` | Skip pre-game events farther out than this |
| `BALANCE_REFRESH_CYCLES` | `60` | Periodic live balance refresh interval in poll cycles |
| `SKIP_LIVE` | `0` | `1` skips all live/in-play candidates |
| `WS_URL` | `ws://127.0.0.1:3402` | Bet placer WebSocket URL |

Hardcoded runtime values to know:

- `require_pinnacle_leg = true` in `main.rs`.
- Poll interval is 5 seconds.
- Book freshness limit is 30 seconds.
- Pre-execution odds drift threshold is 5%.
- Rate limit is currently 200 bets/hour per venue.
- REST API binds to `0.0.0.0:3401`.

## Detection Pipeline

```text
fetch odds
  -> normalize to CanonicalMarket
  -> diff/touch in-memory Book
  -> expire stale entries
  -> group by MarketIdentityKey
  -> find complementary selections
  -> compute sigma
  -> emit ArbCandidate when sigma < 1.0
  -> persist ArbOpportunity
  -> safety checks
  -> paper or live execution
```

Important market matching behavior:

- `MarketIdentityKey.is_live` is always forced to `false` so venues can pair when they disagree on live status.
- The actual live flag still flows through `PriceEntry` and `ArbCandidate`.
- Spread line keys use absolute value so `+1.5` and `-1.5` group together before sign validation.
- Soccer and other 3-way moneylines are blocked.
- Partial-period moneylines are blocked unless `period == "full_game"`.
- Pinnacle alternate markets are filtered out.
- FanDuel "60 Min Moneyline (3 Way)" and similar labels map to `regulation`.

## Execution Flow

Live execution in `live_executor.rs` is sequential:

1. Size the trade with current DB balances.
2. If Pinnacle is involved, quote Pinnacle first.
3. Recalculate sigma with quoted Pinnacle price.
4. Adjust stakes for Pinnacle min/max stake if needed.
5. Place the soft-book leg first.
6. Recalculate sigma after leg 1 fills.
7. Place Pinnacle second.
8. Persist a trade record and update balances.

If leg 1 fails, leg 2 is not placed. If leg 2 fails after leg 1 fills, the engine logs one-sided exposure. Rescue hedge computation exists but is not currently wired into live execution.

## REST API

The engine serves these routes on port `3401`:

| Route | Method | Purpose |
| --- | --- | --- |
| `/api/balances` | GET | Current balances |
| `/api/balances/deposit` | POST | Add funds to a venue balance |
| `/api/balances/withdraw` | POST | Remove funds from a venue balance |
| `/api/balances/reset` | POST | Reset balances to initial values |
| `/api/trades` | GET | Last 200 trades |
| `/api/arbs` | GET | Last 200 detected arbs |
| `/api/stats` | GET | Aggregate stats |
| `/api/settings` | GET | Current execution settings |
| `/api/settings` | POST | Replace execution settings |
| `/api/reset` | POST | Clear trades/arbs and reset balances |
| `/ws` | WS | Realtime event stream |

Example:

```powershell
Invoke-RestMethod http://localhost:3401/api/settings
Invoke-RestMethod http://localhost:3401/api/balances
Invoke-RestMethod http://localhost:3401/api/trades
Invoke-RestMethod http://localhost:3401/api/stats
```

## Database

SQLite file:

```text
engine/data/trades.db
```

Tables are auto-created in `db.rs`:

| Table | Purpose |
| --- | --- |
| `book_balances` | Venue balances and initial balances |
| `paper_trades` | Paper and live trade records |
| `arb_opportunities` | Detected candidate log |
| `settings` | Mutable execution settings |

## Supported Venues

| Venue | Role | Live Placement |
| --- | --- | --- |
| DraftKings | Soft book | Yes, through Patchright browser cookies |
| FanDuel | Soft book | Yes, through Patchright context requests |
| Pinnacle | Sharp book | Yes, through browser-extracted session token and direct API calls |

## Supported Sports

NBA, NHL, MLB, NFL, NCAAB, NCAAF, soccer through FanDuel event type, EPL, La Liga, Serie A, Bundesliga, Ligue 1, UCL, MLS, and UFC.

## Current Gaps

- `cargo check` passes, but `cargo test` currently fails to compile because test fixtures are stale after model changes.
- No JavaScript test runner is configured.
- Rescue hedging is implemented but not wired into live execution.
- Treasury/reservation accounting exists but is unused.
- State machine and structured trade logging exist but are unused.
- No WebSocket reconnect loop from engine to bet placer.
- No alerting for one-sided exposure.
- No settlement reconciliation or actual realized P&L tracking.
- FanDuel balance reading is DOM-scraped and fragile.
- DraftKings wager client version headers are hardcoded.

## License

Private. All rights reserved.
