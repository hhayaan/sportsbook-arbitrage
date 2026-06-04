# Setup

This guide is for running the current local Windows project.

## Prerequisites

Install:

- Rust stable with Cargo;
- Node.js 20 or newer;
- npm;
- Microsoft Edge;
- Windows MSVC build tools.

Check versions:

```powershell
rustc --version
cargo --version
node --version
npm --version
```

## Repository Layout

```text
C:\dev\sportsbook-arbitrage
  dk-proxy.js
  bet-placer\
    index.js
    package.json
    src\sportsbooks\
  data\
    profiles\
    cookies\
  engine\
    Cargo.toml
    src\
    data\
      trades.db
      events.jsonl
      bets\
      naked-legs.json
  docs\
```

There are two runtime data roots:

- root `data\` is used by the Node browser profiles and cookie snapshots;
- `engine\data\` is used by the Rust engine when you run Cargo from `engine\`.

## Install Dependencies

Node:

```powershell
cd C:\dev\sportsbook-arbitrage\bet-placer
npm install
```

Rust:

```powershell
cd C:\dev\sportsbook-arbitrage\engine
cargo check
```

Create runtime directories:

```powershell
cd C:\dev\sportsbook-arbitrage
New-Item -ItemType Directory -Force data\cookies | Out-Null

cd C:\dev\sportsbook-arbitrage\engine
New-Item -ItemType Directory -Force data | Out-Null
```

## Required Processes

Start the DraftKings proxy:

```powershell
cd C:\dev\sportsbook-arbitrage
node .\dk-proxy.js
```

Start the bet placer:

```powershell
cd C:\dev\sportsbook-arbitrage\bet-placer
npm start
```

The bet placer starts:

- WebSocket on `ws://localhost:3402`;
- authenticated Pinnacle odds bridge on `http://localhost:3404`;
- Microsoft Edge browser contexts for DraftKings, FanDuel, and Pinnacle.

Log in manually in the opened sportsbook browsers. Authenticated Pinnacle scanning requires the Pinnacle browser to be logged in.

The current Patchright browser channel is hardcoded to `msedge` in `bet-placer\src\browser-manager.js`. Chrome is not required for the current runtime unless that channel is reverted.

## Run Scan/Paper Mode

Default scan mode uses authenticated Pinnacle odds, so keep the bet placer running.

```powershell
cd C:\dev\sportsbook-arbitrage\engine
cargo run --bin arb-engine
```

To intentionally scan Pinnacle guest odds instead:

```powershell
cd C:\dev\sportsbook-arbitrage\engine
$env:PIN_ODDS_SOURCE = "guest"
cargo run --bin arb-engine
```

Guest mode is not the recommended live-testing source.

## Run Live Mode With $5 Soft-Book Cap

Use this after DK proxy and bet placer are already running and all sportsbook browsers are logged in.

```powershell
cd C:\dev\sportsbook-arbitrage\engine
$env:LIVE_MODE = "1"
$env:PIN_ODDS_SOURCE = "auth"
$env:FIXED_STAKE = "5"
$env:MAX_STAKE = "5"
$env:MIN_ARB_PCT = "0.5"
cargo run --bin arb-engine
```

Meaning:

- `FIXED_STAKE=5` asks fixed mode to target a $5 soft-book stake.
- `MAX_STAKE=5` caps the non-Pinnacle soft-book leg.
- Pinnacle hedge size can exceed $5 when needed, but it is bounded by authenticated quote max, Pinnacle balance, and profitability checks.

Optional pregame-only testing:

```powershell
$env:SKIP_LIVE = "1"
```

## One-Line PowerShell Run

PowerShell can still set env variables and run Cargo on one line:

```powershell
cd C:\dev\sportsbook-arbitrage\engine; $env:LIVE_MODE="1"; $env:PIN_ODDS_SOURCE="auth"; $env:FIXED_STAKE="5"; $env:MAX_STAKE="5"; $env:MIN_ARB_PCT="0.5"; cargo run --bin arb-engine
```

The variables apply to that PowerShell process. They remain set in the same terminal until changed or closed.

## Startup Confirmation

The engine should print:

- `Arb Engine V6`;
- settings including `pin_odds_source=auth`;
- DraftKings proxy endpoint;
- Pinnacle auth odds bridge;
- WebSocket connected to bet placer in live mode;
- live balances for logged-in books.

If live mode cannot connect to the bet placer at startup, it falls back to paper execution.

## Validation Commands

Rust:

```powershell
cd C:\dev\sportsbook-arbitrage\engine
cargo test
```

Node syntax:

```powershell
cd C:\dev\sportsbook-arbitrage
node --check .\dk-proxy.js
node --check .\bet-placer\index.js
node --check .\bet-placer\src\browser-manager.js
node --check .\bet-placer\src\sportsbooks\draftkings.js
node --check .\bet-placer\src\sportsbooks\fanduel.js
node --check .\bet-placer\src\sportsbooks\pinnacle.js
```

Docs formatting:

```powershell
cd C:\dev\sportsbook-arbitrage
git diff --check
```

## Active Environment Variables

Common Rust variables:

| Variable | Use |
| --- | --- |
| `LIVE_MODE` | `1` enables live execution through bet placer |
| `PIN_ODDS_SOURCE` | `auth` by default, `guest` only for guest Pinnacle scan |
| `FIXED_STAKE` | fixed-mode target soft stake |
| `MAX_STAKE` | non-Pinnacle soft-book stake cap |
| `MIN_ARB_PCT` | minimum arb percent |
| `SKIP_LIVE` | `1` skips live markets |
| `ODDS_FETCH_TIMEOUT_MS` | per-source fetch deadline |
| `LIVE_ALLOWED_PAIRS` | optional live placement pair allowlist, for example `fanduel/pinnacle`; scanning remains unchanged |
| `PIN_FILL_TIMEOUT_MS` | Pinnacle fill-report wait |
| `FILL_TIMEOUT_MS` | non-Pinnacle fill-report wait |
| `MAX_RESCUE_LOSS` | optional rescue-loss cap |
| `ATTEMPT_TELEMETRY_DIR` | telemetry root |
| `NAKED_LEGS_FILE` | naked-leg ledger path |
| `FAMILY_DEDUP_FILE` | family dedup state path |
| `FAMILY_DEDUP_TTL_MS` / `FAMILY_DEDUP_TTL_SECS` | hard-lock and repeat-window TTL, default 6 hours |
| `FAMILY_DEDUP_SUCCESS_COOLDOWN_MS` / `FAMILY_DEDUP_SUCCESS_COOLDOWN_SECS` | successful-fill cooldown, default 30 seconds |
| `FAMILY_DEDUP_SUCCESS_MAX_REPEATS` | successful repeats allowed after first fill, default 3 |
| `DETECTOR_TELEMETRY` | `1` enables detector JSONL diagnostics |
| `DETECTOR_DIAGNOSTICS` | alias for `DETECTOR_TELEMETRY` |
| `DETECTOR_SNAPSHOT_LIMIT` | max candidate/near-miss snapshots per poll when detector telemetry is enabled |
| `DETECTOR_NEAR_MISS_SIGMA_MAX` | highest sigma written as near-miss snapshot, default `1.02` |

Common Node variables:

| Variable | Use |
| --- | --- |
| `WS_PORT` | bet placer WebSocket port |
| `PIN_ODDS_PORT` | authenticated Pinnacle odds bridge port |
| `DK_PROXY_PORT` | DK proxy port |
| `DK_PROXY_FETCH_TIMEOUT_MS` | DK proxy upstream deadline |
| `FD_WRITE_TIMEOUT_MS` | FanDuel cashout write timeout |

## Current Operating Notes

- Keep stakes small while validating live behavior.
- Check `engine\data\bets\<attempt_id>.jsonl` after any incident.
- For FD/PIN or DK/PIN scan diagnosis, run one scan with `DETECTOR_TELEMETRY=1` and inspect `engine\data\events.jsonl`.
- Cashout attempts may appear in telemetry without a matching terminal line.
- If Pinnacle says accepted but Rust times out, inspect the fill report path immediately; accepted Pinnacle IDs must reach Rust as parseable `bet_id`/`purchase_id`.
- If a high-percent arb appears, inspect `pin_venue_market_id`; full-game Pinnacle moneyline must be `s;0;m`, not `s;<nonzero>;m`.
