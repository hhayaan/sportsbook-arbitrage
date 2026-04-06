# Complete Session Handoff — Sports Arbitrage Bot

## What This Document Is

Full context dump of the entire development history. A new Claude session should be able to read this document, CLAUDE.md, and the docs/ folder and pick up exactly where we left off. Every bug, fix, design decision, and architectural choice is documented here.

---

## Project Goal

Build an automated sports arbitrage betting system that:
1. Scans public odds APIs across DraftKings, FanDuel, and Pinnacle (15+ sports) every 5 seconds
2. Maintains an in-memory order book with 30-second freshness and diff-based updates
3. Detects cross-venue arbitrage opportunities (sigma < 1.0 = guaranteed profit)
4. Places both legs automatically via stealth browser sessions (DK/FD) and direct HTTP (Pinnacle)
5. Runs autonomously with a 14-layer safety filter chain including two-stage sigma revalidation
6. Supports both pre-match and live (in-play) game arbitrage
7. Region: Ontario, Canada (CA-ON)

---

## Architecture

```
sports-arb-h2/
├── engine/           ← Rust (Tokio + Axum) — odds scanning, arb detection, execution
├── bet-placer/       ← Node.js — WebSocket server, Patchright browser automation
├── dk-proxy.js       ← DK TLS proxy for Windows (port 3403)
└── data/             ← SQLite DB, browser profiles, cookies
```

### Communication Flow
```
Engine (port 3401) ──WS──► Bet Placer (port 3402) ──API──► FD/DK/PIN Sportsbooks
Engine ──HTTP──► DK Proxy (port 3403) ──HTTPS──► DraftKings odds API
Engine ──HTTP──► FanDuel odds API (direct, .ca domain for Ontario)
Engine ──HTTP──► Pinnacle odds API (direct, guest API, no auth)
```

### Three Processes
1. **DK Proxy** (`node dk-proxy.js`, port 3403) — Rust's reqwest can't complete TLS handshake with DK's CDN on Windows. Node's `fetch()` handles it. Uses `.fetch()` not `https.get` (the latter had the same issue).
2. **Bet Placer** (`node index.js`, port 3402) — Launches 3 Patchright Chrome windows (DK, FD, PIN). Receives bet commands from engine via WebSocket. Routes to correct adapter. Returns fill reports.
3. **Engine** (`cargo run`, port 3401) — Polls odds, detects arbs, executes trades. REST API for settings/balances/stats. WebSocket broadcaster for real-time events.

---

## History

### Pre-H1: Chrome Extension (Abandoned)
AWS server + Chrome MV3 extension via WebSocket. Content scripts in FD/DK tabs. Abandoned — fragile, service worker sleep, CSP hacking.

### H1: Patchright + Node.js Engine
Single Node.js process. NBA moneylines only. DK + FD only. 30s scan interval. No safety systems. Key fix: FD `placeBet` switched from `page.evaluate(fetch())` to `context.request.post()` to bypass PerimeterX (403 DSC-0015).

### V3: Partner's Rust Engine
Rust + Tokio. 15+ sports, 3 venues. In-memory order book. Safety systems. SQLite. Redis pub/sub for live execution. Dashboard. Pinnacle odds-only (no bet placement).

### H2: Merged Project (Current)
Combined V3 Rust engine with H1 Node.js/Patchright automation. Key decisions:
- Redis → WebSocket (eliminated dependency)
- Added Pinnacle bet placement (built from HAR capture)
- Removed dashboard (settings via env vars/API)
- Removed venue substitution (rarely profitable)

### V4 Merge: Partner's Safety Improvements
Merged 17 safety improvements from partner's V4 engine update into H2.

---

## Bet Placement Methods

### DraftKings (`draftkings.js`)
- **Transport**: `page.evaluate(fetch())` with `credentials: "include"` — browser cookies sent automatically
- **Flow**: getSelectionInfo → calculateBets (returns `token` + `calculationId`) → placeBet → waitForConfirmation (poll up to 15s)
- **Host**: `gaming-ca-on.draftkings.com/api/wager/v1`
- **Selection ID**: `0ML{marketId}_{outcomeIndex}` (e.g., `0ML84036019_3`)
- **Auth**: HttpOnly browser cookies via `credentials: "include"`
- **XPP filter**: DraftKings Crowns (XPP) filtered from balance — not real currency

### FanDuel (`fanduel.js`)
- **Transport**: `context.request.post()` — Node-side HTTP, bypasses PerimeterX CORS
- **Flow**: getMarketPrices → implyBets (returns `betReference`) → placeBet
- **Hosts**: `sib.on.sportsbook.fanduel.ca` (imply), `spb.on.sportsbook.fanduel.ca` (place), `smp.on.sportsbook.fanduel.ca` (prices)
- **Selection ID**: `{marketId}:{selectionId}` (e.g., `801.161172464:60427`)
- **Auth**: JWT from `localStorage["session-sportsbook"]`, geo token from `localStorage["location_token"]`, PerimeterX cookies via `context.cookies()`
- **Currency**: Hardcoded to CAD (Ontario)
- **Critical**: `handicap` must always be included in runner object, even for moneylines (use `0`)

### Pinnacle (`pinnacle.js`) — Hybrid Adapter
- **Transport**: Direct `fetch()` with tokens extracted from browser localStorage. Browser stays open as token source only — not used for HTTP requests. Faster than `context.request`.
- **Token refresh**: Auto-reads `localStorage["Main:User"]` every 5 minutes for `deviceId` and `token`
- **Flow**: quote (returns `marketId` + `price` + `minRiskStake`/`maxRiskStake`) → place (returns `PENDING_ACCEPTANCE`) → poll `GET /bets?requestIds=` until accepted (~7-8s)
- **Host**: `api.arcadia.pinnacle.com/0.1`
- **Selection ID**: `{matchupId}:{marketKey}:{designation}` (e.g., `1627197304:s;0;m:home`)
- **Auth**: `x-api-key` (static: `CmX2KcMrXuFmNg6YFbmTxE0y9CIrOi0R`), `x-device-uuid`, `x-session`
- **Min stake**: ~$0.81 CAD (varies per market, obtained from quote response)

---

## 14-Layer Safety Filter Chain (in order)

1. **3-way sport ML block** — Soccer moneylines have 3 outcomes (home/draw/away). 2-leg arb only covers 2, third loses both. Sports: SOCCER, EPL, LA_LIGA, SERIE_A, BUNDESLIGA, LIGUE_1, UCL, MLS.
2. **Partial-period ML block** — Only full_game moneylines allowed. F3/F5/1st half MLs can tie.
3. **Per-venue vig check** — Each venue's implied probability sum must be 0.95-1.25. Outside = misclassified market (3-way as 2-way, wrong period, doubleheader collision).
4. **Spread line sign check** — Requires opposite signs (-1.5 vs +1.5). Same sign = both can lose.
5. **Require Pinnacle leg** — `require_pinnacle_leg = true`. DK-FD arbs blocked (lost money on those).
6. **Circuit breaker** — Pauses trading on cumulative loss or high rejection rate.
7. **Rate limiter** — Per-venue sliding window bet limit.
8. **Odds drift** — Rejects if odds moved >5% since detection.
9. **>15% arb warning** — Logs `[HIGH%]` but doesn't block.
10. **Time-to-start** — Skips events beyond `MAX_HOURS_TO_START` (default 6h) to prevent capital lockup.
11. **Balance-aware stake scaling** — Scales down stakes to fit venue balances instead of rejecting.
12. **Quote sigma revalidation** — After Pinnacle quote returns current price, recalculates sigma. Aborts if >= 1.0.
13. **Post-fill sigma revalidation** — After leg 1 fills at actual odds, recalculates sigma. Aborts leg 2 if >= 1.0 (one-sided exposure better than guaranteed loss).
14. **60-second dedup cooldown** — Prevents re-trading same event/market/venue pair.

---

## Execution Order

Soft book FIRST, Pinnacle second. Pre-flight quote runs before any bets.

1. Engine detects arb
2. If Pinnacle is involved: send `quote_bet` to get min/max stakes and current price
3. **Quote sigma revalidation** — abort if sigma >= 1.0 at quoted price
4. Adjust stakes to fit Pinnacle min/max while maintaining arb ratio
5. **Place leg 1** (FD or DK — soft book). If rejected = zero exposure.
6. **Post-fill sigma revalidation** — abort leg 2 if sigma >= 1.0 at actual fill odds
7. **Place leg 2** (Pinnacle). Almost never rejects.
8. Both filled → calculate P&L, record trade, update balances

---

## Engine Configuration

### Env Vars
| Var | Default | Description |
|-----|---------|-------------|
| `LIVE_MODE` | `0` | `1` = real bets |
| `FIXED_STAKE` | `25` | $ per leg |
| `MAX_STAKE` | `50` | Cap per leg |
| `MIN_ARB_PCT` | `0.5` | Min profit % to trigger |
| `STAKE_MODE` | `fixed` | `fixed` or `proportional` |
| `PROPORTIONAL_PCT` | `5` | % of balance |
| `ENABLED` | `true` | `false` = scan only |
| `MAX_HOURS_TO_START` | `6` | Max hours before event start |
| `BALANCE_REFRESH_CYCLES` | `60` | Periodic balance sync interval (cycles) |

### Hardcoded Values
- `require_pinnacle_leg = true` — cannot be changed via env var
- `POLL_INTERVAL_SECS = 5`
- `MAX_DATA_AGE_MS = 30,000` (30s)
- Odds drift threshold: 5%
- Vig check range: 0.95-1.25
- Fill timeout: 20s per leg
- Token refresh: 5 min (Pinnacle)
- Keepalive: 5 min (FD/DK browser reload)

---

## In-Memory Order Book

Two-level HashMap: `MarketIdentityKey → VenueSelectionKey → PriceEntry`

### MarketIdentityKey
- `event_id` — canonical: `{SPORT}:{DATE}T{HOUR}:{TEAMA}-vs-{TEAMB}` (alphabetically sorted, hour included for doubleheader disambiguation)
- `market_type` — `moneyline`, `spread`, `alt_spread`, `total`, `alt_total`, `team_total`, `player_prop`
- `line_key` — `format!("{:.1}", line)`, uses `abs()` for spreads so +1.5 and -1.5 group together
- `period` — `full_game`, `first_half`, `second_half`, `first_3_innings`, `regulation`, etc. (Pinnacle periods 0-9 mapped)
- `is_live` — **always false** (DK hardcodes false, PIN uses true for same live game — using actual value breaks pairing)
- `player_key` — empty for non-player markets, `"lastname:proptype"` for player props

### Diff-Based Updates
Only entries where odds actually changed get written to the book. Unchanged entries get `book.touch()` to refresh their timestamp and prevent expiry. This was a critical bug fix — without touch(), the book dropped from ~12,000 to ~700 entries after the first cycle.

---

## Bugs Fixed (Chronological)

1. **FD DSC-0015** — `page.evaluate(fetch())` blocked by PerimeterX. Fixed with `context.request.post()`. (H1)
2. **Draw/3-way moneyline** — 40-70% fake arbs pairing team win with draw on soccer. Fixed: draw normalization in `team_names.rs` + `THREE_WAY_SPORTS` blocking in `arb_detector.rs`.
3. **DK proxy TLS** — `https.get` had same TLS issue as Rust reqwest. Rewrote to use `fetch()`.
4. **FD currency** — Showed "USD" instead of "CAD". Hardcoded to CAD for Ontario.
5. **DK XPP** — DraftKings Crowns showing in balance. Filtered out (`currencyCode !== "XPP"`).
6. **Pinnacle token extraction** — Was scanning all localStorage keys. Fixed to read `localStorage["Main:User"]` directly.
7. **FD URL domain** — Was `.com`, should be `.ca` for Ontario. Fixed in odds_fetcher.rs.
8. **Spread line matching** — 34-38% fake NHL spread arbs. Same-side bets paired as complementary. Fixed: `line_key` uses `abs()` for spreads.
9. **Book expiry** — Dropped from 12K to 700 entries. Diff system didn't refresh timestamps on unchanged entries. Fixed: `book.touch()`.
10. **Pinnacle min stake** — Below-minimum stakes rejected, causing one-sided exposure. Fixed: pre-flight quote with stake adjustment.
11. **Pinnacle alternates** — Alternate spread lines at different odds paired against main lines. Fixed: `isAlternate` filter.
12. **is_live mismatch** — DK=false, PIN=true for same live game. Fixed: always false in MarketIdentityKey.
13. **Doubleheader collision** — Same teams, same day, different times got same event ID. Fixed: `extract_date()` includes hour.

---

## User Accounts
- DraftKings: `hhayaan` (Ontario)
- FanDuel: `alveebaba` (Ontario)
- Pinnacle: `HA1963952` (Ontario, pinnacle.ca)

---

## MSVC Build Configuration
The engine requires Visual Studio 2026 MSVC linker. Path is hardcoded in `engine/.cargo/config.toml`:
- Linker: `C:\Program Files\Microsoft Visual Studio\18\Community\VC\Tools\MSVC\14.50.35717\bin\Hostx64\x64\link.exe`
- Windows SDK: `10.0.26100.0`
- If the linker path changes (VS update), update `engine/.cargo/config.toml`

---

## Known Issues

| Issue | Status |
|-------|--------|
| **One-sided exposure** | Mitigated — pre-flight quote + two-stage sigma revalidation prevent most cases. Post-fill sigma can intentionally cause it. Rescue hedger exists (`rescue_hedger.rs`) but not wired up. |
| DK TLS on Windows | Mitigated via dk-proxy.js |
| FD sessions expire ~13 min | Keepalive refreshes every 5 min, may still expire |
| DK/FD max stake limits unknown | Soft books may limit max bet amounts per user. Partner working on detection. |
| DK-FD arbs blocked | `require_pinnacle_leg=true` — these lost money |
| NFL/NCAAF DK fetches fail | Offseason — expected, harmless |
| PIN MLB occasional 401 | Retries next cycle |
| Unused engine modules | `treasury.rs`, `state_machine.rs`, `rescue_hedger.rs`, `policy.rs`, `trades.rs` — written for future use, not wired into main loop. Zero runtime cost. |

---

## How to Run

### Paper Trading (scan only, no browsers needed)
```powershell
# Terminal 1: DK Proxy
cd C:\dev\sportsbook_arbitrage; node dk-proxy.js

# Terminal 2: Engine
cd C:\dev\sportsbook_arbitrage\engine; $env:ENABLED="false"; cargo run
```

### Live Trading
```powershell
# Terminal 1: DK Proxy
cd C:\dev\sportsbook_arbitrage; node dk-proxy.js

# Terminal 2: Bet Placer (opens 3 Chrome windows)
cd C:\dev\sportsbook_arbitrage\bet-placer; node index.js
# Log in to DK, FD, PIN. Type "balance" to verify all 3.

# Terminal 3: Engine
cd C:\dev\sportsbook_arbitrage\engine; $env:LIVE_MODE="1"; $env:FIXED_STAKE="0.50"; $env:MAX_STAKE="2"; $env:MIN_ARB_PCT="0.5"; $env:ENABLED="true"; cargo run
```

### REST API (while engine is running)
```powershell
Invoke-RestMethod http://localhost:3401/api/settings
Invoke-RestMethod http://localhost:3401/api/balances
Invoke-RestMethod http://localhost:3401/api/trades
Invoke-RestMethod http://localhost:3401/api/stats
```

---

## WebSocket Protocol (Engine ↔ Bet Placer)

### Engine → Bet Placer
```json
{"type":"place_bet","attempt_id":"uuid-L1","venue":"fanduel","venue_market_id":"801.161172464:60427","selection":"BOS","stake":12.50,"expected_odds":1.95}
{"type":"quote_bet","attempt_id":"uuid-Q","venue":"pinnacle","venue_market_id":"1627197304:s;0;m:home"}
{"type":"get_balances"}
{"type":"ping"}
```

### Bet Placer → Engine
```json
{"type":"fill_report","attempt_id":"uuid-L1","status":"filled","accepted_stake":12.50,"accepted_odds":1.95,"error_message":null}
{"type":"quote_result","attempt_id":"uuid-Q","status":"ok","min_stake":0.81,"max_stake":10180.31,"price":1.95}
{"type":"balances","balances":[{"venue":"draftkings","balance":6.48},{"venue":"fanduel","balance":19.65},{"venue":"pinnacle","balance":25.00}]}
{"type":"balance_update","venue":"pinnacle","balance":24.54}
{"type":"pong"}
```

---

## Partner's Projects (Reference)
- `C:\dev\sports-arb-h1` — Original H1 Node.js project (source of automation code)
- `C:\dev\arb-engine-v3` — Partner's V3 Rust engine (source of initial engine)
- `C:\dev\sports-arb-v4` — Partner's V4 engine update (source of safety improvements)
- `C:\dev\sportsbook_browser_automation` — Standalone browser automation (pre-H1)

---

## Critical Rules for Next Session

1. **Start bet placer BEFORE engine** — engine connects WS on startup, fails if bet placer isn't running
2. **DK proxy must be running** — Rust can't fetch DK directly on Windows
3. **GeoComply PLC must be running** for FanDuel geo verification
4. **User must be logged in** to all 3 sportsbooks in the browser windows before starting engine
5. **Never commit** browser profiles, cookies, or auth tokens
6. **Region is Ontario (CA-ON)** — all API endpoints use Ontario/Canadian domains
7. **FD uses `.ca` domain**, not `.com`
8. **Pinnacle URL is `pinnacle.ca`**, not `pinnacle.com`
9. **is_live is always false** in MarketIdentityKey — intentional, do not change
10. **require_pinnacle_leg is hardcoded true** — DK-FD arbs lost money
11. **Odds are locked at placement** — payouts don't change regardless of future odds movement
12. **The codebase must stay LEAN** — no bloat, no unnecessary abstractions, minimal latency
13. **MSVC linker path** in `engine/.cargo/config.toml` must match installed Visual Studio version

---

## Future Roadmap

### Immediate
- Detect and handle DK/FD maximum stake limits (soft book user limiting)
- Wire up rescue hedger for one-sided exposure recovery

### Phase 1: Reliability
- Session expired detection (401) with auto-refresh or alert
- WebSocket reconnection on disconnect
- Auto-retry on transient failures

### Phase 2: More Sportsbooks
- BetMGM, ESPN BET/TheScore, PointsBet, Betano
- Each needs: HAR capture, API reverse engineering, adapter implementation

### Phase 3: More Markets
- Player prop arb detection (data model ready, parsing ready, not tested)
- Re-enable Pinnacle alternates when DK/FD alt scanning is added
- DNB/Asian Handicap parsing for soccer (2-way soccer markets)

### Phase 4: Infrastructure
- Deploy to cloud for 24/7 operation
- Alerting (Telegram/Discord) for trades, errors, exposure
- Multi-region support (US states beyond Ontario)
