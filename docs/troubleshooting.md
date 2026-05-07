# Troubleshooting (Current State)

This document records current operational issues plus historical fixes that should remain part of the project memory.

## Current Status Snapshot

Known current state:

- Rust engine serves REST and dashboard WebSocket on `0.0.0.0:3401`.
- DraftKings proxy listens on `localhost:3403`.
- Bet placer listens on `localhost:3402`.
- Bet placer also exposes authenticated Pinnacle odds snapshots on `localhost:3404`.
- SQLite is opened at `data/trades.db` relative to the engine process working directory.
- Live execution requires an engine startup connection to the bet placer.
- Default Pinnacle scanning requires the bet placer HTTP bridge and a logged-in Pinnacle browser. The old guest feed is opt-in with `PIN_ODDS_SOURCE=guest`.
- Odds drift gate is currently 5% per leg in `main.rs`.
- `engine/config.toml` is stale and not the authoritative runtime configuration.
- `cargo check` and `cargo test` pass, with warnings from unused legacy/helper modules.

## Startup Checklist

Use this order for paper mode:

```powershell
cd C:\dev\sportsbook-arbitrage
node .\dk-proxy.js
```

```powershell
cd C:\dev\sportsbook-arbitrage\bet-placer
npm start
```

Log in to Pinnacle if using the default authenticated Pinnacle odds source. To use the older guest Pinnacle feed instead, set `$env:PIN_ODDS_SOURCE = "guest"` before starting the engine.

```powershell
cd C:\dev\sportsbook-arbitrage\engine
cargo run --bin arb-engine
```

Use this order for live mode:

```powershell
cd C:\dev\sportsbook-arbitrage
node .\dk-proxy.js
```

```powershell
cd C:\dev\sportsbook-arbitrage\bet-placer
npm start
```

```powershell
cd C:\dev\sportsbook-arbitrage\engine
$env:LIVE_MODE = "1"
cargo run --bin arb-engine
```

Confirm the engine log says the WebSocket connected to the bet placer before assuming live betting is active.

## Engine Cannot Fetch DraftKings Odds

Symptoms:

- DraftKings market count is zero.
- DK proxy terminal has no requests.
- Rust logs mention DraftKings fetch failures.

Checks:

```powershell
cd C:\dev\sportsbook-arbitrage
node .\dk-proxy.js
```

Then open:

```text
http://localhost:3403/dk/42648
```

Expected behavior:

- The proxy returns JSON for NBA when the public DK endpoint is healthy.
- The proxy logs a `/dk/{leagueId}` request.
- Slow or dead DraftKings leagues should now fail around `ODDS_FETCH_TIMEOUT_MS`, default `2500`, instead of holding the whole Rust poll cycle for 15 seconds.
- The proxy also aborts its upstream DraftKings fetch after `DK_PROXY_FETCH_TIMEOUT_MS`, default `2500`.

Notes:

- Rust tries `http://localhost:3403/dk/{leagueId}` first.
- Rust then falls back to the direct DraftKings public URL.
- The proxy exists because Node `fetch` has worked more reliably with DraftKings TLS/CDN behavior on Windows.

## Engine Starts but Live Bets Do Not Place

Likely causes:

- `LIVE_MODE` was not set in the PowerShell session used to run Cargo.
- Bet placer was not running before engine startup.
- Engine failed initial WebSocket connection and fell back to paper execution.
- Sportsbook browser sessions are logged out or geo/session tokens are stale.
- Default Pinnacle scanning cannot reach the bet placer's `3404` HTTP bridge.

Checks:

- In the bet placer terminal, confirm `[WS] Engine connected`.
- In the engine terminal, confirm `WebSocket connected to bet placer`.
- In the bet placer CLI, run `status` and `balance`.
- Confirm DraftKings, FanDuel, and Pinnacle browser windows are logged in.

Important current limitation: the engine does not reconnect to the bet placer after startup. Restart the engine after starting or restarting the bet placer.

## Live Mode Falls Back to Paper

Current behavior in `main.rs`:

- If `LIVE_MODE=1`, the engine tries to connect to `WS_URL`.
- If that connection fails, `ws_conn` is `None`.
- Later execution branches to the paper executor when `ws_conn` is `None`.

This is intentional current behavior but operationally dangerous if missed. Treat WebSocket connection logs as mandatory live-mode confirmation.

## One-Sided Exposure After Leg 1

Current live execution places one leg first and then the second leg. It reduces risk by:

- placing the softer book first when Pinnacle is involved,
- quoting Pinnacle before either leg is placed,
- validating DK/FD before soft-leg risk,
- rechecking Pinnacle immediately before the soft leg is sent,
- rechecking sigma after leg 1 fills.

Current recovery behavior:

- If leg 2 rejects or post-fill math kills the hedge, runtime enters recovery.
- Recovery can quote original/adjacent Pinnacle markets, compare cashout offer versus rescue loss, attempt DK/FD cashout, and persist unresolved exposure to `data/naked-legs.json`.
- The monitor reloads naked legs after restart and periodically tries status/cashout/relist recovery.

Remaining gap:

- Settlement ingest and the full Hunter cashout retry schedule are not merged yet.
- Manual monitoring is still required during live-money tests.

Response:

- Watch live logs and `data/bets/<attempt_id>.jsonl` during any real-money run.
- Keep `FIXED_STAKE` and `MAX_STAKE` small while testing. `MAX_STAKE` caps the soft-book side only; Pinnacle hedge size is quote/balance/profit bounded.
- Be prepared to manually hedge on the opposite outcome.

## Cargo Test And Warnings

Current known issue:

- `cargo check` passes.
- `cargo test` passes.
- The crate still emits warnings from unused legacy/helper modules.

Current expected warnings include:

- unused legacy helpers in `trades.rs`, `treasury.rs`, `state_machine.rs`, and `policy.rs`;
- unused model fields retained for API/model compatibility;
- older player-prop helpers that are not in current market scope.

Until tests are refreshed, use:

```powershell
cd C:\dev\sportsbook-arbitrage\engine
cargo check
cargo test
```

Treat tests as compile/unit health, not as a complete replay suite for live sportsbook incidents.

## `engine/config.toml` Disagrees with Runtime

Symptoms:

- `config.toml` says `require_pinnacle_leg = false`, but runtime requires Pinnacle.
- `config.toml` says max odds drift is 0.03, but runtime uses 0.05.
- `config.toml` has venue limits/min stakes that do not match the main loop behavior.

Current source of truth:

- hardcoded runtime values in `main.rs` and `config.rs`,
- SQLite `settings` table,
- startup environment overrides.

Treat `engine/config.toml` as historical/stale until it is either rewired or removed.

## REST API Exposed on Local Network

Current behavior:

- Axum binds to `0.0.0.0:3401`.
- CORS allows any origin.
- There is no auth.

Risk:

- Anyone who can reach the port can call balance/settings/reset endpoints.

Mitigation:

- Run only on a trusted local machine/network.
- Use Windows firewall rules if needed.
- Do not port-forward this service.

## Cookie Save Fails on Quit

Symptom:

- Bet placer exits with an error when saving cookies.

Cause:

- `BrowserManager.saveCookies` writes under root `data/cookies`, but does not currently create that directory.

Fix:

```powershell
cd C:\dev\sportsbook-arbitrage
New-Item -ItemType Directory -Force data\cookies | Out-Null
```

## FanDuel Balance Looks Wrong

Current implementation:

- Reads username from a localStorage/JWT path when possible.
- Scrapes visible `span`, `div`, `p`, and `button` text.
- Picks the largest visible dollar-looking value.

This can misread promotional balances, unrelated page values, or stale UI. Verify FanDuel balances manually before live execution.

## Pinnacle Quote Fails

Symptoms:

- Live candidate involving Pinnacle is skipped.
- Logs show `quote failed`, `No selection in quote`, `x-session`, or 401/403 style errors.

Checks:

- Pinnacle browser is open and logged in.
- The bet placer terminal reports a Pinnacle token source, commonly `localStorage:Main:User`.
- Browser storage contains a device ID and session token in one of the adapter's searched auth objects.
- The event/market still exists.
- `venue_market_id` is in `{matchupId}:{marketKey}:{designation}` form.

Current flow:

- The bet placer uses direct Node `fetch`, not Patchright `context.request`, for Pinnacle betting.
- Tokens are refreshed from the browser before each Pinnacle API request.
- A failed Pinnacle quote skips the arb before any leg is placed.

## Pinnacle Auth Odds Fail Or Look Stale

Symptoms:

- Logs show `[PIN] ... fetch failed` while `pin_odds_source=auth` appears in the startup settings.
- No Pinnacle-backed candidates appear.
- You see old guest-source behavior after expecting authenticated odds.

Checks:

- Bet placer is running and printed `[PIN-ODDS] Listening on http://localhost:3404`.
- Pinnacle browser is logged in at `https://www.pinnacle.ca`.
- `PIN_ODDS_SOURCE` is unset or set to `auth`, `authenticated`, `logged-in`, or `logged_in`.
- `PIN_AUTH_ODDS_BASE_URL` points to the right local bridge if you changed `PIN_ODDS_PORT`.

Intentional guest fallback:

```powershell
$env:PIN_ODDS_SOURCE = "guest"
```

Do not use guest mode for live testing unless you explicitly want to compare behavior; guest Pinnacle prices have shown stale lines during testing.

## DraftKings Placement Fails

Common causes:

- Browser logged out.
- Selection expired or market suspended.
- `calculateBets` rejects the selection.
- `placeBets` returns a non-200 response.
- Confirmation stays `LiveDelay` until timeout.

Current flow:

1. `selectioninfo`
2. `calculateBets`
3. `placeBets`
4. `getPurchases`

DraftKings placement uses browser-page `fetch` with credentials included. If DK changes client headers or betslip payload shape, the adapter may need to be updated.

## FanDuel Placement Fails

Common causes:

- Auth token missing from `session-sportsbook`.
- Geo packet missing from `geolocation_document.jwt`; `location_token.jwt` is only a legacy fallback.
- PX cookies missing or expired.
- `implyBets` returns a non-success response.
- Market selection ID no longer maps to a current runner.

Current flow:

1. `getMarketPrices`
2. `implyBets`
3. `placeBet`

FanDuel placement uses Patchright `context.request` with generated headers and browser cookies.

## Keepalive Reload Interference

The bet placer reloads each sportsbook page every 5 minutes:

```text
KEEPALIVE_INTERVAL_MS = 5 * 60 * 1000
```

This keeps sessions warm, but a reload can interfere with manual inspection or a sensitive browser state. Be cautious around live placement windows.

## Odds Drift Rejections

Current runtime drift gate:

```text
verify_both_legs(..., 0.05)
```

This rejects candidates when either current book price differs from detection-time price by more than 5%. If many candidates are rejected for drift:

- Check source freshness.
- Check whether a venue is updating more slowly than others.
- Check whether the event is live/in-play.
- Consider raising `MIN_ARB_PCT` rather than loosening drift while testing.

## High Arb Percent Warnings

The current loop warns on very high `arb_pct`, but does not automatically reject solely for being high. Very large arbs are often caused by:

- mismatched teams,
- stale market data,
- spread sign problems,
- period mismatches,
- three-way market misclassification,
- source parsing drift.

Review the normalized event ID, market type, period, line, selections, and venue market IDs before trusting unusually high opportunities.

## Historical Fixes Preserved

These are previous project issues that were fixed or partially fixed and should not be lost from project memory.

### DraftKings Proxy Switched to Node Fetch

Older proxy code used lower-level HTTPS handling that was less reliable on Windows. `dk-proxy.js` now uses Node `fetch`, which has been more reliable against DraftKings' CDN/TLS behavior.

### Draw Arbs Blocked

The detector now blocks two-leg moneyline arbs for soccer-style three-way sports and rejects `draw` selections in moneyline complements. This fixed false arbs where home/away both looked profitable but draw was a third live outcome.

### Partial-Period Moneylines Blocked

Partial-period moneylines are skipped because ties/pushes are common and two-leg home/away treatment is unsafe.

### Regulation Period Detection Added

DK/FD labels containing `60 min`, `3 way`, or `3-way` are mapped to `regulation`. Pinnacle period `6` is also mapped to `regulation`. This reduces false mismatches between 60-minute/three-way markets and full-game markets.

### Spread Sign Validation Added

Spread market identity groups by absolute line value, but detection now requires opposite line signs. This prevents both-favorite or both-underdog spread pairs from being treated as complementary.

### Alternate Pinnacle Lines Filtered

Pinnacle alternate-market handling was tightened so alternate lines do not pollute primary market comparisons.

### Team Name Normalization Expanded

`team_names.rs` contains broad normalization for pro, college, soccer, and fighter names, plus fallbacks. Canonical IDs sort team codes to match the same game across venues with different home/away string conventions.

### Adapter-Ready Venue Market IDs Added

The normalized market model carries venue-specific IDs needed by live placement:

- DraftKings selection IDs
- FanDuel `{marketId}:{selectionId}`
- Pinnacle `{matchupId}:{marketKey}:{designation}`

### Pinnacle Quote Preflight Added

Live execution now quotes Pinnacle before any leg is placed, validates quoted sigma, and adjusts stakes for Pinnacle min/max limits.

### Balance Sync Added

Live startup fetches real balances through the bet placer and writes them into SQLite. Live fills can also send best-effort `balance_update` messages back to Rust.

### Paper Trading Kept Separate from Live Placement

Paper execution remains a local simulation in SQLite. Live placement is isolated behind the bet placer WebSocket.

## Quick Reference

Ports:

```text
3401 Rust REST/dashboard WebSocket
3402 Node bet placer WebSocket
3403 DraftKings public odds proxy
3404 Bet placer authenticated Pinnacle odds bridge
```

Key URLs:

```text
http://localhost:3401/api/balances
http://localhost:3401/api/trades
http://localhost:3401/api/arbs
http://localhost:3401/api/settings
ws://localhost:3401/ws
ws://localhost:3402
http://localhost:3403/dk/42648
http://localhost:3404/health
http://localhost:3404/pinnacle/snapshot/246
```

Low-stake live test environment:

```powershell
cd C:\dev\sportsbook-arbitrage\engine
$env:LIVE_MODE = "1"
$env:FIXED_STAKE = "5"
$env:MAX_STAKE = "5"
$env:MIN_ARB_PCT = "0.5"
$env:SKIP_LIVE = "1"
cargo run --bin arb-engine
```
