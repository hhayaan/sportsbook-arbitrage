# Architecture (Current State)

This document describes the current architecture as implemented in the codebase. The project is a local sportsbook arbitrage system for Ontario accounts. It continuously collects odds, normalizes them into a shared market model, detects two-leg arbitrage candidates, and can either simulate trades in paper mode or place live bets through browser-backed sportsbook sessions.

The system is intentionally local-first:

- Rust owns odds ingestion, normalization, detection, sizing, recovery orchestration, persistence, REST, and dashboard WebSocket broadcasts.
- Node owns browser automation, authenticated live bet placement, and the authenticated Pinnacle odds snapshot bridge.
- SQLite is the only database.
- There is no external queue, cloud database, or remote worker.

## Process Topology

```text
Public odds APIs
  DraftKings public odds
  FanDuel public odds
        |
        v
Rust arb engine (engine/)
  polls every 5 seconds
  normalizes markets and teams
  detects arbs
  sizes trades
  writes SQLite
  serves REST and dashboard WebSocket
        |
        | ws://127.0.0.1:3402 for live betting
        | http://127.0.0.1:3404 for authenticated PIN odds by default
        v
Node bet placer (bet-placer/)
  Patchright persistent Chrome profiles
  DraftKings adapter
  FanDuel adapter
  Pinnacle adapter
        |
        v
Authenticated sportsbook sessions
```

Pinnacle is the important exception to the public-odds wording above. DraftKings and FanDuel scanning still use public sportsbook endpoints. Pinnacle scanning now defaults to the logged-in browser-backed snapshot exposed by the bet placer. The old guest Arcadia path still exists, but only when `PIN_ODDS_SOURCE=guest` or `PIN_ODDS_SOURCE=public` is explicitly set.

There is one supporting Node process:

```text
Rust DK fetcher -> http://localhost:3403/dk/{leagueId} -> dk-proxy.js -> DraftKings public odds
```

`dk-proxy.js` exists because the Rust `reqwest` client has had TLS trouble with DraftKings' CDN on Windows. The Rust fetcher tries the local proxy first and then falls back to the direct DraftKings URL.

## Runtime Processes

| Process | Default port | Entry point | Role |
| --- | ---: | --- | --- |
| Rust engine | 3401 | `engine/src/main.rs` | Odds polling, detection, trade execution, REST API, dashboard WebSocket |
| Bet placer | 3402 | `bet-placer/index.js` | WebSocket bridge from Rust to authenticated browser betting flows |
| DraftKings proxy | 3403 | `dk-proxy.js` | Fetches DraftKings public odds with Node `fetch` |
| Pinnacle auth odds bridge | 3404 | `bet-placer/index.js` | Local HTTP bridge from Rust to the logged-in Pinnacle snapshot endpoints |

When the Rust engine is run from `engine/`, its SQLite path is `engine/data/trades.db`. The code opens `data/trades.db` relative to the current working directory.

## Rust Engine

The engine binary is configured in `engine/Cargo.toml` as `arb-engine`. The package name still contains the older `arb-engine-v3` label, but the runtime described by these docs is the current post-Hunter-first-wave engine.

Major Rust modules:

| File | Current responsibility |
| --- | --- |
| `main.rs` | Process startup, settings overrides, polling loop, live/paper branch, REST routes |
| `config.rs` | Hardcoded sports, league IDs, public odds hosts, poll timing, venue list |
| `odds_fetcher.rs` | DraftKings/FanDuel public odds ingestion, Pinnacle authenticated-or-guest odds ingestion, and canonical market creation |
| `team_names.rs` | Team/fighter/selection normalization and canonical event ID generation |
| `models.rs` | Core data structures, settings, API models, dashboard WebSocket events |
| `arb_detector.rs` | Two-leg arb detection and market safety filters |
| `odds_verifier.rs` | Pre-execution odds drift check against the in-memory book |
| `stake_optimizer.rs` | Sigma-based stake sizing, soft-book cap handling, and balance-aware scaling |
| `paper_executor.rs` | Paper execution with instant theoretical settlement |
| `live_executor.rs` | Live WebSocket command protocol, authenticated prefire checks, live two-leg execution, rescue/cashout/naked-leg recovery |
| `execution_math.rs` | Authenticated Pinnacle quote validation and hedge stake math based on actual soft fills |
| `attempt_telemetry.rs` | JSONL global and per-attempt telemetry writer |
| `naked_legs.rs` | Durable unresolved-exposure ledger |
| `pin_yank.rs` | Temporary hard-yank blacklist for unavailable Pinnacle markets |
| `db.rs` | SQLite schema, migrations, balances, trades, arbs, settings |
| `api.rs` | Axum REST handlers and dashboard WebSocket upgrade |
| `ws.rs` | Broadcast channel used by dashboard clients |
| `circuit_breaker.rs` | Loss/rejection circuit breaker type |
| `rate_limiter.rs` | Per-venue sliding window limiter type |
| `state_machine.rs` | Rich attempt-state model, currently not wired into the main execution path |
| `rescue_hedger.rs` | Older standalone rescue hedge calculator; live recovery now uses `live_executor.rs` and `execution_math.rs` |
| `policy.rs` | Multi-gate policy helper, currently not the primary runtime gate path |
| `treasury.rs` | Free/reserved/in-flight balance model, currently not wired into runtime accounting |
| `trades.rs` | Structured trade logging helpers, partially available but not the primary persistence path |

## Odds Ingestion

`odds_fetcher::fetch_all` spawns concurrent fetches for all configured sports and venues.

Current venue behavior:

- DraftKings odds use `http://localhost:3403/dk/{leagueId}` first, then fall back to `https://sportsbook-nash.draftkings.com/sites/CA-ON-SB/api/sportscontent/dkcaon/v1/leagues/{leagueId}`.
- FanDuel odds use Ontario content-managed-page endpoints with `_ak=FhMFpcPWXMeyZxOx`. Normal pages use `page=CUSTOM`; soccer uses `page=SPORT&eventTypeId=1`.
- Pinnacle odds default to `PIN_ODDS_SOURCE=auth`. Rust calls the local bet-placer endpoint `GET /pinnacle/snapshot/{leagueId}` on `PIN_AUTH_ODDS_BASE_URL` or `http://127.0.0.1:3404`, and the bet placer fetches logged-in `https://api.arcadia.pinnacle.com/0.1/leagues/{leagueId}/matchups` plus `markets/straight`.
- The previous guest Arcadia scan remains available with `PIN_ODDS_SOURCE=guest` or `PIN_ODDS_SOURCE=public`; it uses `https://guest.api.arcadia.pinnacle.com/0.1`.
- Each odds request has a short deadline controlled by `ODDS_FETCH_TIMEOUT_MS`, default `2500`. This prevents one slow league, such as a failing DraftKings NCAAB endpoint, from holding the whole poll cycle for the old 15-second HTTP timeout.

Supported configured sports include NBA, NHL, MLB, NFL, NCAAB, NCAAF, major soccer leagues, MLS, and UFC. NFL is configured for DraftKings and FanDuel only because the attempted Pinnacle league path is disabled in code after returning 401 during earlier guest-source testing.

## Canonical Market Model

Each public price becomes a `CanonicalMarket`:

- `event_id`
- `sport`
- `league`
- `start_time_utc`
- `market_type`
- `selection`
- `line`
- `period`
- `is_live`
- `decimal_odds`
- `american_odds`
- `venue`
- `venue_market_id`
- `venue_event_id`
- `received_at_ms`

Canonical event IDs are generated by `team_names::canonical_event_id`. Team codes are normalized, sorted alphabetically, and combined with sport plus date/hour when available:

```text
NBA:2026-03-15T19:BOS-vs-GSW
```

The current implementation sorts teams so the same game matches across venues even when one source says "away at home" and another source reverses the order. This also means the `A-vs-B` portion is not necessarily away-vs-home.

Market identity uses `MarketIdentityKey`:

```text
event_id + market_type + line_key + period + is_live + player_key
```

For spreads, `line_key` uses the absolute value so `-1.5` and `+1.5` land in the same market bucket. The detector then separately verifies the two spread legs have opposite signs.

## Detection Strategy

The detector is deliberately conservative:

- Only two-leg complements are considered.
- Three-way soccer moneyline markets are skipped because two-leg home/away arbs ignore draw.
- Partial-period moneylines are skipped because ties/push semantics make them unsafe.
- Draw selections are rejected in moneyline complements.
- Totals require `over` vs `under`.
- Spreads require different selections and opposite line signs.
- The current main loop requires at least one Pinnacle leg for execution candidates.
- Candidates are sorted by `arb_pct` descending.

Arb math:

```text
sigma = 1 / leg1_decimal_odds + 1 / leg2_decimal_odds
arb_pct = 1 - sigma
roi = 1 / sigma - 1
```

An arb exists only when `sigma < 1.0`.

## Execution Path

The engine has paper and live execution branches.

Paper mode:

- Uses current SQLite balances.
- Sizes stakes with `stake_optimizer::compute_stakes_with_balances`.
- Deducts both stakes.
- Immediately credits stake plus theoretical profit share back to both venues.
- Persists a `paper_trades` row with status `executed`.

Live mode:

- Connects to the bet placer at `WS_URL` or `ws://127.0.0.1:3402`.
- Fetches real balances with `get_balances`.
- If the initial WebSocket connection fails, the engine logs a warning and continues using paper execution even when `LIVE_MODE=1`.
- Places the softer book first and Pinnacle second when Pinnacle is involved.
- Runs a Pinnacle `quote_bet` preflight before either leg is placed.
- Runs authenticated DK/FD `validate_soft` before soft-leg risk.
- Runs a short Pinnacle prefire recheck immediately before sending the soft leg.
- Uses actual filled soft stake and odds to recompute the Pinnacle hedge.
- Adjusts Pinnacle stake to quoted min/max and checks Pinnacle balance and post-rounding profitability.
- Aborts if quote sigma, validation sigma, prefire sigma, or post-leg-1 sigma closes the arb.
- Persists a trade with status `executed_live` only after both legs fill.

If leg 1 fills and leg 2 cannot be placed, live execution now enters a recovery path. It can compare soft-book cashout against rescue loss, try Pinnacle original/adjacent-line rescue where a Pinnacle hedge ID is available, attempt DK/FD cashout, and persist unresolved exposure to `data/naked-legs.json`. Manual monitoring is still required during real-money runs because settlement ingest and a full Hunter-style retry schedule are not yet implemented.

## Runtime Gates

The current main loop applies these runtime gates before execution:

- Candidate must be detected from fresh data (`MAX_DATA_AGE_MS`, hardcoded 30000 ms).
- Candidate must include Pinnacle because `require_pinnacle_leg` is currently `true` in `main.rs`.
- Candidate odds must still be present in the in-memory book.
- Odds drift must be at or below 5% per leg.
- The first stake sizing pass caps only the non-Pinnacle soft-book stake with `settings.max_stake`; Pinnacle hedge size is later bounded by authenticated quote max, Pinnacle balance, and profitability checks.
- `SKIP_LIVE=1` skips live markets.
- Pregame events farther out than `MAX_HOURS_TO_START` are skipped. Default is 24 hours.
- Duplicate trade keys are skipped after a trade has already been executed.
- `settings.enabled` must be true.
- `settings.min_arb_pct` is enforced inside paper/live executors.
- Stake plans must meet minimum capital/profit constraints and venue balances.

Types for circuit breaking and rate limiting are present. In the current `main.rs`, the circuit breaker is instantiated with a $500 loss threshold over 3600 seconds, a 50% rejection threshold over 20 records, and a 60 second cooldown. The per-venue rate limiter is initialized at 200 bets/hour. Treat these as local runtime safeguards, not exchange-grade risk management.

## Bet Placer

The Node bet placer launches one persistent Chrome context per live book:

- DraftKings
- FanDuel
- Pinnacle

It uses Patchright with persistent profiles under `data/profiles/{book}`. Cookies are saved under `data/cookies/{book}-data.json` when the process exits through the CLI `quit`/`exit` flow. The current `saveCookies` path assumes `data/cookies` already exists.

The bet placer listens on `ws://localhost:3402` by default and accepts:

- `place_bet`
- `validate_soft`
- `quote_bet`
- `get_balances`
- `get_bet_status`
- `cashout`
- `ping`

It also starts a local HTTP server on `http://localhost:3404` by default:

- `GET /health`
- `GET /pinnacle/snapshot/{leagueId}`

Adapter transports:

- DraftKings betting calls run inside the DraftKings page via `page.evaluate(fetch(..., credentials: "include"))`.
- FanDuel betting calls use Patchright `context.request` with extracted auth, geo, and PX headers. FanDuel cashout writes use browser-page `fetch` because stale `x-geo-packet` failures were observed on the older request path.
- Pinnacle betting and authenticated odds snapshot calls use direct Node `fetch` with `x-api-key`, `x-device-uuid`, and `x-session` headers extracted from the logged-in Pinnacle browser.

The keepalive timer reloads each running sportsbook page every 5 minutes. That keeps sessions warm, but it can race with manual/browser state or an active betting flow.

## Persistence

SQLite schema in `db.rs`:

- `book_balances`
- `paper_trades`
- `arb_opportunities`
- `settings`

Additional runtime files:

- `data/events.jsonl` for global attempt telemetry, or `{ATTEMPT_TELEMETRY_DIR}/events.jsonl` when overridden.
- `data/bets/<attempt_id>.jsonl` for per-attempt lifecycle telemetry.
- `data/naked-legs.json` for unresolved soft-leg exposure, or `NAKED_LEGS_FILE` when overridden.

Defaults are seeded with `INSERT OR IGNORE`, so existing local DB values are preserved.

Default settings:

| Setting | Default |
| --- | ---: |
| `enabled` | `true` |
| `min_arb_pct` | `0.5` |
| `max_stake` | `50` |
| `stake_mode` | `fixed` |
| `fixed_stake` | `25` |
| `proportional_pct` | `5` |

Environment variables can override settings at startup:

- `FIXED_STAKE`
- `MAX_STAKE`
- `MIN_ARB_PCT`
- `STAKE_MODE`
- `PROPORTIONAL_PCT`
- `ENABLED`

Additional runtime environment variables:

- `LIVE_MODE`
- `SKIP_LIVE`
- `MAX_HOURS_TO_START`
- `BALANCE_REFRESH_CYCLES`
- `NAKED_LEG_MONITOR_CYCLES`
- `ODDS_FETCH_TIMEOUT_MS`
- `WS_URL`
- `PIN_ODDS_SOURCE`
- `PIN_AUTH_ODDS_BASE_URL`
- `QUOTE_TIMEOUT_MS`
- `PIN_FILL_TIMEOUT_MS`
- `FILL_TIMEOUT_MS`
- `SOFT_PREFIRE_VALIDATION_ENABLED`
- `SOFT_PREFIRE_VALIDATION_TIMEOUT_MS`
- `SOFT_PREFIRE_MAX_SLIPPAGE_PCT`
- `PIN_PREFIRE_RECHECK_ENABLED`
- `PIN_PREFIRE_RECHECK_TIMEOUT_MS`
- `PIN_YANK_BLACKLIST_MS`
- `RESCUE_PIN_HARD_YANK_THRESHOLD`
- `RESCUE_QUOTE_TIMEOUT_MS`
- `MAX_RESCUE_LOSS`
- `BET_STATUS_TIMEOUT_MS`
- `CASHOUT_TIMEOUT_MS`
- `NAKED_LEG_PIN_RECHECK_QUOTE_TIMEOUT_MS`
- `ATTEMPT_TELEMETRY_DIR`
- `NAKED_LEGS_FILE`
- `WS_PORT`
- `DK_PROXY_PORT`
- `DK_PROXY_FETCH_TIMEOUT_MS`
- `PIN_ODDS_PORT`
- `FD_WRITE_TIMEOUT_MS`

`engine/config.toml` is currently legacy/stale relative to the runtime code. The live settings path is SQLite plus startup environment overrides, and most strategy/policy values in `config.toml` are not loaded by `main.rs`.

## Dashboard API

The Rust engine serves Axum on `0.0.0.0:3401` with permissive CORS:

- `GET /api/balances`
- `POST /api/balances/deposit`
- `POST /api/balances/withdraw`
- `POST /api/balances/reset`
- `GET /api/trades`
- `GET /api/arbs`
- `GET /api/stats`
- `GET /api/settings`
- `POST /api/settings`
- `POST /api/reset`
- `GET /ws`

There is no authentication or authorization on this API. Run it only on a trusted local network, or bind/firewall it before exposing it beyond the local machine.

## Current Shortcomings

- The engine REST API binds to `0.0.0.0` and has no auth.
- Live WebSocket reconnect is not implemented after startup.
- If live mode cannot connect to the bet placer during startup, execution falls back to paper mode.
- Default Pinnacle scanning now requires the bet placer to be running and Pinnacle to be logged in; set `PIN_ODDS_SOURCE=guest` only when intentionally using the older public guest scan.
- `engine/config.toml` is stale and should not be treated as authoritative runtime configuration.
- Rescue/cashout/naked-leg recovery is wired into live execution, but settlement ingest and the full Hunter retry schedule are not merged.
- `state_machine.rs`, `treasury.rs`, `policy.rs`, and `trades.rs` are present but not wired into the primary execution flow.
- `cargo test` currently passes, but the crate still has warnings from unused legacy/helper modules.
- The FanDuel balance adapter scrapes the visible page and chooses the largest dollar-looking value, which is fragile.
- The bet placer keepalive reload can interfere with active browser state.
- Cookie saving assumes the `data/cookies` directory exists.
