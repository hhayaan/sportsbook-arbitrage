# CLAUDE.md

## Project Overview
Automated sports arbitrage system. Rust engine scans odds from DraftKings, FanDuel, and Pinnacle every 5 seconds across 15+ sports. Detects cross-venue arbs (sigma < 1.0 = guaranteed profit). Places both legs via Node.js bet placer using Patchright stealth browsers.

## Architecture
```
Engine (Rust, port 3401) --WS--> Bet Placer (Node.js, port 3402) --API--> Sportsbooks
Engine --HTTP--> DK Proxy (port 3403) --HTTPS--> DraftKings odds API
Engine --HTTP--> FanDuel odds API (direct)
Engine --HTTP--> Pinnacle odds API (direct)
```

## Running (Live Mode)
```powershell
# T1: DK Proxy
cd C:\dev\sports-arb-h2; node dk-proxy.js
# T2: Bet Placer (opens 3 Chrome windows — log in to all, type "balance" to verify)
cd C:\dev\sports-arb-h2\bet-placer; node index.js
# T3: Engine
cd C:\dev\sports-arb-h2\engine; $env:LIVE_MODE="1"; $env:FIXED_STAKE="1"; $env:MAX_STAKE="2"; $env:MIN_ARB_PCT="0.1"; cargo run
```

## Engine Env Vars
| Var | Default | Description |
|-----|---------|-------------|
| `LIVE_MODE` | `0` | `1` = real bets |
| `FIXED_STAKE` | `25` | $ per leg |
| `MAX_STAKE` | `50` | Cap per leg |
| `MIN_ARB_PCT` | `0.5` | Min profit % |
| `STAKE_MODE` | `fixed` | `fixed` or `proportional` |
| `PROPORTIONAL_PCT` | `5` | % of balance |
| `ENABLED` | `true` | `false` = scan only |

## Key Files
| File | Purpose |
|------|---------|
| `engine/src/main.rs` | Entry point, polling loop, execution |
| `engine/src/odds_fetcher.rs` | DK/FD/PIN odds fetchers |
| `engine/src/arb_detector.rs` | Sigma calc, cross-venue detection |
| `engine/src/live_executor.rs` | WebSocket bet execution |
| `engine/src/models.rs` | Order book, data structures |
| `engine/src/config.rs` | Sports, leagues, API hosts |
| `bet-placer/index.js` | WS server, adapter dispatch |
| `bet-placer/src/sportsbooks/draftkings.js` | DK adapter (page.evaluate) |
| `bet-placer/src/sportsbooks/fanduel.js` | FD adapter (context.request.post) |
| `bet-placer/src/sportsbooks/pinnacle.js` | PIN adapter (context.request) |
| `dk-proxy.js` | DK TLS workaround |

## Bet Placement Methods
- **DraftKings**: `page.evaluate(fetch())` with `credentials: "include"` (browser cookies)
- **FanDuel**: `context.request.post()` (Node-side HTTP, bypasses PerimeterX)
- **Pinnacle**: `context.request.post()` with `x-api-key`, `x-device-uuid`, `x-session` headers

## Selection ID Formats
- DK: `0ML{marketId}_{outcomeIndex}` (e.g., `0ML84036019_3`)
- FD: `{marketId}:{selectionId}` (e.g., `801.161172464:60427`)
- PIN: `{matchupId}:{marketKey}:{designation}` (e.g., `1627197304:s;0;m:home`)

## Execution Order
FD first (soft book, more likely to reject) -> then DK or PIN. If leg 1 fails, leg 2 is never placed.

## Sports
NBA, NHL, MLB, NFL, NCAAB, NCAAF, EPL, La Liga, Serie A, Bundesliga, Ligue 1, UCL, MLS, UFC

## Critical Rules
- Start bet placer BEFORE engine (engine connects WS on startup)
- DK proxy must be running (Rust can't fetch DK directly on Windows)
- GeoComply PLC must be running for FanDuel
- User must be logged in to all 3 sportsbooks before starting engine
- Never commit browser profiles, cookies, or auth tokens
- Region: Ontario (CA-ON)

## Known Issues
- DK TLS on Windows -> use dk-proxy.js (uses fetch, not https.get)
- FD sessions expire ~13 min, keepalive refreshes every 5 min
- True DK-FD arbs are rare; most arbs involve Pinnacle
- NFL/NCAAF DK fetches fail in offseason (harmless)
- Draw selections normalized to "draw" in team_names.rs, excluded from moneyline arbs in arb_detector.rs
- FD_STATES config still set to ["nj", "mi"] — should be ["on"] for Ontario (pending V3 merge)

## REST API (port 3401)
```powershell
Invoke-RestMethod http://localhost:3401/api/settings          # GET settings
Invoke-RestMethod http://localhost:3401/api/balances           # GET balances
Invoke-RestMethod http://localhost:3401/api/trades             # GET trades
Invoke-RestMethod http://localhost:3401/api/stats              # GET stats
```
