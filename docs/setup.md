# Setup & Installation

## Prerequisites

- **Rust toolchain** — `rustup` with `stable-x86_64-pc-windows-msvc`
- **Node.js 18+** — for bet placer and DK proxy
- **Visual Studio 2022** — Community or Build Tools with C++ workload (MSVC linker)
- **GeoComply PLC** — must be running for FanDuel geo verification (Ontario)
- **Sportsbook accounts** — DraftKings, FanDuel, Pinnacle (all Ontario/CA)

## Installation

```powershell
# Install Node dependencies
cd C:\dev\sportsbook-arbitrage\bet-placer; npm install

# Build Rust engine (first build downloads + compiles dependencies, takes a few minutes)
cd C:\dev\sportsbook-arbitrage\engine; cargo build
```

## Running — Paper Trading

No browsers needed. Scans odds and simulates trades.

```powershell
# Terminal 1: DK Proxy
cd C:\dev\sportsbook-arbitrage; node dk-proxy.js

# Terminal 2: Engine
cd C:\dev\sportsbook-arbitrage\engine; cargo run
```

## Running — Live Trading

```powershell
# Terminal 1: DK Proxy
cd C:\dev\sportsbook-arbitrage; node dk-proxy.js

# Terminal 2: Bet Placer (opens 3 Chrome windows)
cd C:\dev\sportsbook-arbitrage\bet-placer; node index.js
```

**Pause here** — Log in to DraftKings, FanDuel, and Pinnacle in the browser windows. Type `balance` to verify all three sessions.

```powershell
# Terminal 3: Engine (live mode with test stakes)
cd C:\dev\sportsbook-arbitrage\engine; $env:LIVE_MODE="1"; $env:FIXED_STAKE="1"; $env:MAX_STAKE="2"; $env:MIN_ARB_PCT="0.1"; cargo run
```

## Verifying Sessions

In the bet placer terminal:
```
> balance
[draftkings] hhayaan
  CAD: $6.92
[fanduel] alveebaba
  CAD: $19.65
[pinnacle] HA1963952
  CAD: $3932.00
```

## Bet Placer CLI

| Command | Description |
|---------|-------------|
| `balance [book]` | Check sportsbook balance |
| `info <book> <id>` | Get selection/odds info |
| `bet <book> <id> <stake>` | Place single bet (REAL MONEY) |
| `status` | Show WS connection and browser status |
| `quit` | Save cookies, close browsers, exit |

## Engine Settings via PowerShell

Change settings at launch with env vars:

```powershell
$env:LIVE_MODE="1"
$env:FIXED_STAKE="5"
$env:MAX_STAKE="10"
$env:MIN_ARB_PCT="0.3"
$env:STAKE_MODE="proportional"
$env:PROPORTIONAL_PCT="2"
cargo run
```

Or change settings while the engine is running via the REST API:

```powershell
# View current settings
Invoke-RestMethod http://localhost:3401/api/settings

# Update settings
Invoke-RestMethod -Method POST -Uri http://localhost:3401/api/settings -ContentType "application/json" -Body '{"enabled":true,"min_arb_pct":0.1,"max_stake":2,"stake_mode":"fixed","fixed_stake":1,"proportional_pct":5}'

# Check balances
Invoke-RestMethod http://localhost:3401/api/balances
```

## Session Persistence

- Browser profiles stored in `data/profiles/{bookId}/`
- Cookies, localStorage, IndexedDB all persist across restarts
- Cookie snapshots saved to `data/cookies/` on `quit`
- If a profile gets corrupted, delete `data/profiles/{bookId}/` and re-login

## GeoComply (FanDuel Only)

FanDuel requires GeoComply PLC to be running for Ontario geo verification. If geo verification fails:

```powershell
taskkill /F /IM PlayerLocationCheck.exe
```

PLC auto-restarts. Then refresh the FanDuel browser window.

## MSVC Linker

The engine's `.cargo/config.toml` hardcodes the path to the MSVC linker. If you get linker errors, find your `link.exe`:

```powershell
Get-ChildItem -Recurse "C:\Program Files\Microsoft Visual Studio" -Filter "link.exe" | Select FullName
```

Update `.cargo/config.toml` with the correct path.
