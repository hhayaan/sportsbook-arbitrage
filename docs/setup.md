# Setup (V5 Current State)

This guide describes the current local Windows setup for the V5 sportsbook arbitrage project.

## Prerequisites

Install:

- Rust stable toolchain with Cargo
- Node.js 20 or newer
- npm
- Google Chrome, because Patchright launches persistent Chrome contexts with `channel: "chrome"`
- Windows MSVC build tools, required by Rust native dependencies

Useful checks:

```powershell
rustc --version
cargo --version
node --version
npm --version
```

## Repository Layout

Important runtime directories:

```text
C:\dev\sportsbook-arbitrage
  dk-proxy.js
  data\
    profiles\        # Patchright persistent browser profiles
    cookies\         # saved cookie snapshots, if created
  bet-placer\
    index.js
    package.json
  engine\
    Cargo.toml
    data\
      trades.db      # SQLite DB when engine is run from engine/
    src\
  docs\
```

There are two different `data` locations:

- `engine/data/trades.db` is the Rust engine SQLite database when running from `engine/`.
- root `data/profiles` and `data/cookies` are used by the Node bet placer browser manager.

## Install Dependencies

From the repository root:

```powershell
cd C:\dev\sportsbook-arbitrage
```

Install Node dependencies for the bet placer:

```powershell
cd C:\dev\sportsbook-arbitrage\bet-placer
npm install
```

Fetch/build Rust dependencies:

```powershell
cd C:\dev\sportsbook-arbitrage\engine
cargo check
```

The current Rust package name in `Cargo.toml` still says `arb-engine-v3`, but the binary is:

```text
arb-engine
```

## Create Local Data Directories

The bet placer creates profile directories automatically, but cookie saving currently assumes the cookie directory exists. Create it up front:

```powershell
cd C:\dev\sportsbook-arbitrage
New-Item -ItemType Directory -Force data\cookies | Out-Null
```

The Rust engine creates/migrates SQLite tables in `engine/data/trades.db` as long as you run it from `engine/`. If `engine/data` does not exist:

```powershell
cd C:\dev\sportsbook-arbitrage\engine
New-Item -ItemType Directory -Force data | Out-Null
```

## Run in Paper Mode

Start the DraftKings proxy:

```powershell
cd C:\dev\sportsbook-arbitrage
node .\dk-proxy.js
```

In a second terminal, start the Rust engine from `engine/`:

```powershell
cd C:\dev\sportsbook-arbitrage\engine
cargo run --bin arb-engine
```

Paper mode is the default when `LIVE_MODE` is unset. The engine still fetches live public odds, but executions are simulated in SQLite.

Dashboard/API base:

```text
http://localhost:3401
```

## Run in Live Mode

Live mode uses authenticated browser sessions. Use small stakes and verify every account manually before enabling.

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

The bet placer launches Chrome windows for DraftKings, FanDuel, and Pinnacle. Log in manually in each browser. The terminal session check should report balances for logged-in books.

Then start the Rust engine in live mode:

```powershell
cd C:\dev\sportsbook-arbitrage\engine
$env:LIVE_MODE = "1"
cargo run --bin arb-engine
```

Recommended cautious live startup:

```powershell
cd C:\dev\sportsbook-arbitrage\engine
$env:LIVE_MODE = "1"
$env:FIXED_STAKE = "1"
$env:MAX_STAKE = "1"
$env:MIN_ARB_PCT = "0.5"
$env:SKIP_LIVE = "1"
cargo run --bin arb-engine
```

Important: if the engine cannot connect to the bet placer at startup, it logs a warning and continues in paper execution mode. Confirm the logs say the WebSocket connected before assuming live placement is active.

## Environment Variables

Rust engine:

| Variable | Meaning | Default |
| --- | --- | --- |
| `LIVE_MODE` | Use live WebSocket execution when `1`/`true` | false |
| `FIXED_STAKE` | Startup override for fixed stake per side setting | SQLite/default |
| `MAX_STAKE` | Startup override for max stake per leg | SQLite/default |
| `MIN_ARB_PCT` | Startup override for minimum arb percent | SQLite/default |
| `STAKE_MODE` | `fixed` or `proportional` | SQLite/default |
| `PROPORTIONAL_PCT` | Startup override for proportional stake percent | SQLite/default |
| `ENABLED` | Startup override for execution toggle | SQLite/default |
| `MAX_HOURS_TO_START` | Skip pregame events farther away than this many hours | 24 |
| `BALANCE_REFRESH_CYCLES` | Live balance refresh interval in poll cycles | 60 |
| `SKIP_LIVE` | Skip in-play markets when `1`/`true` | false |
| `WS_URL` | Bet placer WebSocket URL | `ws://127.0.0.1:3402` |

Node:

| Variable | Meaning | Default |
| --- | --- | --- |
| `WS_PORT` | Bet placer WebSocket server port | 3402 |
| `DK_PROXY_PORT` | DraftKings public odds proxy port | 3403 |

## Runtime Settings

The active engine settings are stored in SQLite and can be changed through:

- `GET /api/settings`
- `POST /api/settings`
- startup environment overrides

Default settings seeded by `db.rs`:

```json
{
  "enabled": true,
  "min_arb_pct": 0.5,
  "max_stake": 50.0,
  "stake_mode": "fixed",
  "fixed_stake": 25.0,
  "proportional_pct": 5.0
}
```

`engine/config.toml` is currently a legacy/stale policy draft. Do not treat it as the authoritative runtime configuration for V5.

## Validation Commands

Rust compile check:

```powershell
cd C:\dev\sportsbook-arbitrage\engine
cargo check
```

Node syntax checks:

```powershell
cd C:\dev\sportsbook-arbitrage
node --check .\dk-proxy.js
node --check .\bet-placer\index.js
node --check .\bet-placer\src\browser-manager.js
node --check .\bet-placer\src\sportsbooks\draftkings.js
node --check .\bet-placer\src\sportsbooks\fanduel.js
node --check .\bet-placer\src\sportsbooks\pinnacle.js
```

Current test caveat: `cargo test` includes stale tests/fixtures from older assumptions, including event ID date/hour behavior and older odds-drift thresholds. Use `cargo check` as the current compile-health command until those tests are refreshed.

## Common Startup Problems

DraftKings returns no data:

- Make sure `dk-proxy.js` is running on port 3403.
- Check for `Port 3403 already in use`.
- The Rust fetcher can fall back to direct DraftKings, but the proxy is the expected Windows path.

Live mode still acts like paper mode:

- Start `bet-placer` before the Rust engine.
- Confirm the engine log says it connected to the bet placer WebSocket.
- Confirm `LIVE_MODE=1` is set in the same PowerShell session that starts Cargo.

Cookie save fails on quit:

- Create `C:\dev\sportsbook-arbitrage\data\cookies`.

FanDuel balance looks wrong:

- The current adapter scrapes visible page text and picks the largest dollar-looking amount.
- Confirm the visible sportsbook page is logged in and showing the expected account state.

Unexpected API exposure:

- The Rust server binds to `0.0.0.0:3401` with permissive CORS and no auth.
- Keep it on a trusted local machine/network or restrict it with firewall/bind changes before wider exposure.
