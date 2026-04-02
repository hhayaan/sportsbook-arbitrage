# Session Handoff — Sports Arbitrage Bot H2

## What This Document Is

Full context dump of the entire development history from H1 through H2. Covers where the project started, what changed at each stage, current state, what works, what was tested, and what's next.

---

## Project Goal

Build an automated sports arbitrage betting system that:
1. Scans public odds APIs across DraftKings, FanDuel, and Pinnacle (15+ sports)
2. Maintains an in-memory order book with 30-second freshness
3. Detects cross-venue arbitrage opportunities (sigma < 1.0 = guaranteed profit)
4. Places both legs automatically via stealth browser sessions
5. Runs autonomously with safety systems (drift check, circuit breaker, rate limiter)

---

## History: Pre-H1 (Chrome Extension — Abandoned)

Before H1, the project used a Chrome extension architecture: an AWS server (`aws-server/server.js`) connected to a Chrome MV3 extension (`bridge-extension/`) via WebSocket. The extension injected content scripts into FanDuel and DraftKings tabs for API bet placement. This was abandoned because it was fragile — service worker sleep, WebSocket latency, CSP header stripping hacks, and required the user to manually load an unpacked extension.

## History: H1 (Patchright + Node.js Engine)

H1 replaced the Chrome extension entirely with Patchright (stealth Playwright fork that bypasses GeoComply automation detection). The entire system ran as a single Node.js process:
- `aws-server/server.js` — Engine: NBA-only moneyline scanning, sigma calc, stake optimization
- `src/browser-manager.js` — Patchright browser lifecycle
- `src/sportsbooks/draftkings.js` — DK adapter (page.evaluate)
- `src/sportsbooks/fanduel.js` — FD adapter (context.request.post)
- `index.js` — CLI + scan/execute loop

**H1 limitations:**
- NBA moneylines only
- Two sportsbooks only (DK + FD)
- 30-second scan interval
- No safety systems
- No persistence or history
- Single-process architecture

### Key Bug Fix in H1
FanDuel's `placeBet` originally used `page.evaluate(fetch(...))` which got blocked by PerimeterX (403 DSC-0015). Fixed by switching to `context.request.post()` (Node-side HTTP). This fix carries forward into H2.

### H1 Test Results
Both legs placed successfully with real money:
- FD: Boston Celtics @1.465 (-215), $2 stake
- DK: ATL Hawks @2.900 (+190), $0.84 stake

---

## History: Arb Engine V3 (Partner's Project)

Separate Rust-based engine built by a partner at `C:\dev\arb-engine-v3`. Key features:
- Rust + Tokio async runtime with 5-second polling
- 15+ sports across 3 venues (DK, FD, Pinnacle)
- Multiple market types (moneyline, spread, total, team_total)
- In-memory order book with diff-based updates
- Safety systems (circuit breaker, rate limiter, odds drift, rescue hedger)
- SQLite persistence
- Axum REST API + WebSocket broadcaster
- Next.js dashboard
- Paper trading + live trading via Redis pub/sub

**V3 limitations:**
- Used Redis for live execution (unnecessary dependency)
- Pinnacle was odds-only (no bet placement)
- Dashboard was clunky
- Venue substitution logic rarely profitable

---

## What We Built in H2

### Combining the Projects
Merged the V3 Rust engine with H1's proven Node.js/Patchright browser automation. Key decisions:

1. **Replaced Redis with WebSocket** — Direct WS connection between engine and bet placer. Eliminates Redis dependency with negligible latency impact (~3ms vs ~0.3ms, dwarfed by 200-500ms sportsbook API calls).

2. **Removed venue substitution** — Pinnacle arbs didn't survive substitution to DK/FD. Kept Pinnacle odds scanning but removed the substitution logic.

3. **Added Pinnacle bet placement** — Built a full Pinnacle adapter from HAR analysis. Quote -> Place -> Poll flow with header-based auth. Cleanest API of the three.

4. **Live balance sync** — Engine fetches real sportsbook balances on startup and after each bet, replacing fake $1000 paper balances.

5. **Removed dashboard** — Next.js dashboard removed from codebase. Engine settings configured via env vars or REST API.

6. **DK proxy fix** — Rewrote dk-proxy.js to use Node's `fetch` instead of `https.get` (which had the same TLS issue as Rust's reqwest).

### Changes Made

| Component | Action | Detail |
|-----------|--------|--------|
| `engine/Cargo.toml` | EDIT | Replaced `redis` with `tokio-tungstenite` |
| `engine/src/live_executor.rs` | REWRITE | Redis pub/sub -> WebSocket client. Added `fetch_balances()`, `drain_balance_updates()` |
| `engine/src/main.rs` | EDIT | WS connection instead of Redis. Env var settings overrides. Live balance sync on startup. Removed venue substitution |
| `engine/src/odds_fetcher.rs` | EDIT | DK proxy port 3402->3403. Pinnacle `venue_market_id` now adapter-ready format |
| `engine/.cargo/config.toml` | EDIT | Fixed MSVC linker path for local machine |
| `bet-placer/` | NEW | WebSocket server + Patchright browsers + DK/FD/PIN adapters |
| `bet-placer/src/sportsbooks/pinnacle.js` | NEW | Full Pinnacle adapter: quote -> place -> poll |
| `dk-proxy.js` | NEW | DK TLS proxy using fetch (port 3403) |
| `dashboard/` | REMOVED | Entirely removed from codebase |

### Testing Results

Successfully placed live arb bets:
- DraftKings: $1.27 CAD wager, $7.30 CAD potential payout
- Pinnacle: $0.73 CAD stake, $7.33 CAD potential payout
- Both legs on the same game, opposite sides — guaranteed ~$7.30 return on $2.00 total stake

---

## Current State

### What Works
- 3-venue odds scanning (DK via proxy, FD direct, PIN direct) every 5 seconds
- 15+ sports: NBA, NHL, MLB, NFL, NCAAB, NCAAF, EPL, La Liga, Serie A, Bundesliga, Ligue 1, UCL, MLS, UFC
- Multi-market: moneyline, spread, total, team_total
- Arb detection across all venue combinations (DK-FD, DK-PIN, FD-PIN)
- Live bet execution via WebSocket to bet placer
- All three adapters working: DraftKings, FanDuel, Pinnacle
- Real balance sync from sportsbooks to engine
- Safety: odds drift verification, circuit breaker, rate limiter
- Engine settings via env vars or REST API
- Session persistence (browser profiles survive restarts)

### User Accounts
- DraftKings: `hhayaan`, ~$6.92 CAD
- FanDuel: `alveebaba`, ~$19.65 CAD
- Pinnacle: `HA1963952`, ~$3,932 CAD

### Config (Current Test Values)
- FIXED_STAKE: $1.00
- MAX_STAKE: $2.00
- MIN_ARB_PCT: 0.1%
- Region: Ontario (CA-ON)

---

## Post-Launch Fixes

### Draw/3-Way Moneyline Bug (Critical)
The engine was detecting 40-70% "arbs" on soccer moneylines that were actually pairing a team win with a draw. Soccer moneylines are 3-way (home/draw/away) — a 2-leg arb only covers 2 of 3 outcomes, leaving the third unhedged (100% loss).

**Root cause**: The team name normalizer (`team_names.rs`) turned "Draw" into `"DRAW"` (uppercased), but the arb detector checked for lowercase `"draw"`.

**Two-layer fix**:
1. `team_names.rs` — `normalize_selection()` now catches "Draw"/"The Draw"/"Tie" and returns consistent lowercase `"draw"` before they reach the team normalizer
2. `arb_detector.rs` — `are_selections_complementary()` excludes `"draw"` from moneyline pairing

### DK Proxy Fix
The original dk-proxy.js used Node's `https.get` which had the same TLS issue as Rust's reqwest. Rewrote to use `fetch()` which handles DK's TLS correctly.

### FanDuel Currency Fix
FD balance was showing "USD" instead of "CAD". Hardcoded to CAD for Ontario region.

### DK XPP (Crowns) Filter
DK was showing XPP (DraftKings Crowns) balance alongside CAD. Filtered out — XPP is not real currency.

### Pinnacle Token Fix
Initial Pinnacle adapter couldn't find session tokens — was scanning all localStorage keys. Fixed to read directly from `localStorage["Main:User"]` which contains `token` and `deviceId`.

### Partner's V3 Engine Improvements (Not Yet Merged)
Partner updated the V3 engine with improvements we should adopt:
- **3-way moneyline sport blocking** — identifies soccer sports by name and skips ALL 2-leg moneyline arbs (better than our draw exclusion)
- **Time-to-start filter** — rejects events >12h away (capital lockup) and <2min from start (suspension risk)
- **Suspicious arb warning** — warns on >15% arbs (likely data mismatches)
- **Post-substitution sigma revalidation** — re-checks arb after venue swap
- **Player prop support** — new market category with player name normalization
- **Epoch date parsing** — prevents silent date-parsing failures
- **FD_STATES corrected** — changed from `["nj", "mi"]` to `["on"]` for Ontario

---

## Known Issues

| Issue | Status |
|-------|--------|
| DK TLS on Windows | Mitigated via dk-proxy.js (using fetch) |
| FD sessions expire ~13 min | Keepalive refreshes every 5 min, may still expire |
| Pinnacle session token extraction | Working via `localStorage["Main:User"]` |
| Paper trades are unrealistic | Paper executor does instant fills at exact odds — only useful for arb detection testing |
| 3-way moneyline arbs | Partially fixed (draw exclusion). V3 has a more robust sport-level blocking approach to merge |
| No time-to-start filter | Events far in the future can lock up capital. V3 fix available to merge |
| No suspicious arb warning | Very high % arbs (likely data errors) not flagged. V3 fix available to merge |
| FD_STATES still set to US | Config has `["nj", "mi"]` instead of `["on"]` for Ontario. V3 fix available |
| NFL/NCAAF DK fetches fail | Offseason — expected, harmless warnings |

---

## Future Roadmap

### Immediate: Merge Partner's V3 Improvements
- Adopt 3-way moneyline sport blocking from V3's `arb_detector.rs`
- Adopt time-to-start filter (>12h and <2min rejection)
- Adopt suspicious arb warning (>15% threshold)
- Fix FD_STATES from `["nj", "mi"]` to `["on"]`
- Add player prop data model fields
- Add epoch date parsing to `team_names.rs`

### Phase 1: Reliability & Recovery
- **Odds changed** — re-fetch, recalculate, retry if still profitable
- **Session expired** — detect 401, auto-refresh or alert
- **Partial fill** — one leg placed, other failed -> rescue hedger (code exists, not wired up)
- **Rate limiting** — back off with exponential delay
- **Token refresh** — FD auth ~13 min, geo ~20 min, PIN session ~6 hours

### Phase 2: Additional Sportsbooks
- **BetMGM** — Uses vnauth + SSO + XSRF token auth
- **ESPN BET / TheScore** — GraphQL API
- **PointsBet**
- **Betano**

Each needs: HAR capture, API reverse engineering, adapter implementation, registration.

### Phase 3: Engine Improvements
- Wire up rescue hedger for one-sided exposure recovery
- Wire up policy gates for pre-trade validation
- Wire up state machine for trade lifecycle tracking
- Wire up structured trade logging
- Kelly criterion stake sizing
- Bet history logging and P&L tracking
- Auto-retry on transient failures
- Player prop arb detection

### Phase 4: Infrastructure
- Deploy engine to cloud (AWS/GCP) for 24/7 operation
- Alerting (Telegram/Discord) for trades, errors, exposure
- Performance monitoring and latency tracking
- Multi-region support (US states beyond Ontario)

---

## How to Run

```powershell
# Terminal 1: DK Proxy
cd C:\dev\sports-arb-h2; node dk-proxy.js

# Terminal 2: Bet Placer
cd C:\dev\sports-arb-h2\bet-placer; node index.js
# Log in to DK, FD, PIN. Type "balance" to verify.

# Terminal 3: Engine (live mode)
cd C:\dev\sports-arb-h2\engine; $env:LIVE_MODE="1"; $env:FIXED_STAKE="1"; $env:MAX_STAKE="2"; $env:MIN_ARB_PCT="0.1"; cargo run
```

---

## Important Notes for Next Session

1. **GeoComply PLC must be running** for FanDuel geo verification
2. **All three browsers must be logged in** before starting the engine in live mode
3. **Start bet placer before engine** — engine connects to WS on startup
4. **DK proxy must be running** — Rust can't fetch DK directly on Windows
5. **True arbs between soft books (DK-FD) are rare** — most arbs involve Pinnacle
6. **Pinnacle is a sharp book** — their odds are closest to true probability, creating more arb opportunities against soft books
7. **Odds are locked at placement** — payouts don't change after the bet is placed, regardless of future odds movement
8. **Engine code has unused safety modules** (treasury, state_machine, rescue_hedger, policy, trades) — written for future use, not wired into main loop yet. They compile out with zero runtime cost.
