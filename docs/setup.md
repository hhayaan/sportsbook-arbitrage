# Setup (Current State)

This guide describes the current local Windows setup for the sportsbook arbitrage project.

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

Default Pinnacle scanning now uses the logged-in Pinnacle snapshot bridge from the bet placer. For normal scan/paper mode with Pinnacle-backed candidates, start the bet placer too:

```powershell
cd C:\dev\sportsbook-arbitrage\bet-placer
npm start
```

Log in to Pinnacle in the opened browser. DraftKings/FanDuel login is only required for live betting, but the bet placer currently launches all three browser profiles.

In another terminal, start the Rust engine from `engine/`:

```powershell
cd C:\dev\sportsbook-arbitrage\engine
cargo run --bin arb-engine
```

Paper mode is the default when `LIVE_MODE` is unset. The engine still fetches live odds, but executions are simulated in SQLite.

If you intentionally want the older Pinnacle guest API scan and do not want to start the bet placer for scanning, set:

```powershell
$env:PIN_ODDS_SOURCE = "guest"
```

That path is kept for reversibility, but the current default is authenticated Pinnacle odds because the guest feed has shown stale prices in live testing.

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
$env:FIXED_STAKE = "5"
$env:MAX_STAKE = "5"
$env:MIN_ARB_PCT = "0.5"
$env:SKIP_LIVE = "1"
cargo run --bin arb-engine
```

`FIXED_STAKE=5` requests a $5 soft-book stake in fixed mode, and `MAX_STAKE=5` caps the non-Pinnacle soft-book leg. Pinnacle's hedge can be larger than $5 when needed to complete the arb; it is bounded by the authenticated Pinnacle quote max, Pinnacle balance, and profitability checks.

Important: if the engine cannot connect to the bet placer at startup, it logs a warning and continues in paper execution mode. Confirm the logs say the WebSocket connected before assuming live placement is active.

## Environment Variables

Rust engine:

| Variable | Meaning | Default |
| --- | --- | --- |
| `LIVE_MODE` | Use live WebSocket execution when `1`/`true` | false |
| `FIXED_STAKE` | Startup override for requested fixed soft-book stake | SQLite/default |
| `MAX_STAKE` | Startup override for max non-Pinnacle soft-book stake | SQLite/default |
| `MIN_ARB_PCT` | Startup override for minimum arb percent | SQLite/default |
| `STAKE_MODE` | `fixed` or `proportional` | SQLite/default |
| `PROPORTIONAL_PCT` | Startup override for proportional stake percent | SQLite/default |
| `ENABLED` | Startup override for execution toggle | SQLite/default |
| `MAX_HOURS_TO_START` | Skip pregame events farther away than this many hours | 24 |
| `BALANCE_REFRESH_CYCLES` | Live balance refresh interval in poll cycles | 60 |
| `NAKED_LEG_MONITOR_CYCLES` | Naked-leg recovery monitor interval in poll cycles | 10 |
| `SKIP_LIVE` | Skip in-play markets when `1`/`true` | false |
| `ODDS_FETCH_TIMEOUT_MS` | Per-request odds fetch deadline for DK/FD/PIN scanning | 2500 |
| `WS_URL` | Bet placer WebSocket URL | `ws://127.0.0.1:3402` |
| `PIN_ODDS_SOURCE` | `auth`/`authenticated`/`logged-in` or `guest`/`public` | `auth` |
| `PIN_AUTH_ODDS_BASE_URL` | Local authenticated Pinnacle snapshot bridge | `http://127.0.0.1:3404` |
| `PIN_FILL_TIMEOUT_MS` | Pinnacle fill-report timeout | 45000 |
| `FILL_TIMEOUT_MS` | Non-Pinnacle fill-report timeout | 20000 |
| `QUOTE_TIMEOUT_MS` | General quote timeout | 10000 |
| `SOFT_PREFIRE_VALIDATION_ENABLED` | Enable DK/FD authenticated soft validation | true |
| `SOFT_PREFIRE_VALIDATION_TIMEOUT_MS` | DK/FD validation timeout | 3500 |
| `PIN_PREFIRE_RECHECK_ENABLED` | Enable final Pinnacle quote before soft risk | true |
| `PIN_PREFIRE_RECHECK_TIMEOUT_MS` | Final Pinnacle quote timeout | 1500 |
| `MAX_RESCUE_LOSS` | Optional max rescue loss cap | unset |
| `ATTEMPT_TELEMETRY_DIR` | Telemetry output root | `data` |
| `NAKED_LEGS_FILE` | Naked-leg ledger path | `data/naked-legs.json` |

Node:

| Variable | Meaning | Default |
| --- | --- | --- |
| `WS_PORT` | Bet placer WebSocket server port | 3402 |
| `PIN_ODDS_PORT` | Bet placer local HTTP port for authenticated Pinnacle odds | 3404 |
| `DK_PROXY_PORT` | DraftKings public odds proxy port | 3403 |
| `DK_PROXY_FETCH_TIMEOUT_MS` | DraftKings proxy upstream fetch deadline; falls back to `ODDS_FETCH_TIMEOUT_MS` if set | 2500 |
| `FD_WRITE_TIMEOUT_MS` | FanDuel browser-page cashout write timeout | 25000 |

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

`engine/config.toml` is currently a legacy/stale policy draft. Do not treat it as the authoritative runtime configuration for the current engine.

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

Current test status: `cargo test` passes, but the crate still emits warnings from unused legacy/helper modules.

## Common Startup Problems

DraftKings returns no data:

- Make sure `dk-proxy.js` is running on port 3403.
- Check for `Port 3403 already in use`.
- The Rust fetcher can fall back to direct DraftKings, but the proxy is the expected Windows path.

Live mode still acts like paper mode:

- Start `bet-placer` before the Rust engine.
- Confirm the engine log says it connected to the bet placer WebSocket.
- Confirm `LIVE_MODE=1` is set in the same PowerShell session that starts Cargo.

No Pinnacle candidates appear:

- By default, make sure `bet-placer` is running and Pinnacle is logged in.
- Check that the bet placer printed `[PIN-ODDS] Listening on http://localhost:3404`.
- If you intentionally want the old guest feed, set `PIN_ODDS_SOURCE=guest` before starting the engine.

Cookie save fails on quit:

- Create `C:\dev\sportsbook-arbitrage\data\cookies`.

FanDuel balance looks wrong:

- The current adapter scrapes visible page text and picks the largest dollar-looking amount.
- Confirm the visible sportsbook page is logged in and showing the expected account state.

Unexpected API exposure:

- The Rust server binds to `0.0.0.0:3401` with permissive CORS and no auth.
- Keep it on a trusted local machine/network or restrict it with firewall/bind changes before wider exposure.
