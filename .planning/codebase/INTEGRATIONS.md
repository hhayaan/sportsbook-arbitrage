# External Integrations

**Analysis Date:** 2026-04-09

## APIs & External Services

**Sportsbook Odds APIs (read — engine polls every 5s):**

- **DraftKings** — odds fetched via local DK proxy (Windows TLS workaround)
  - Odds endpoint: `https://sportsbook-nash.draftkings.com/sites/CA-ON-SB/api/sportscontent/dkcaon/v1/leagues/{leagueId}`
  - Proxied via: `http://localhost:3403/dk/{leagueId}` (`dk-proxy.js`)
  - Auth: none required for odds (public API, User-Agent header only)
  - Selection info: `https://sportsbook-nash.draftkings.com/api/sdinfo/dkcaon/v1/selectioninfo`
  - Fetcher: `engine/src/odds_fetcher.rs` → `fetch_draftkings()`

- **FanDuel** — odds fetched directly by engine (no proxy needed)
  - Odds endpoint: `https://smp.on.sportsbook.fanduel.ca/...` (Ontario region)
  - Auth: `x-application: FhMFpcPWXMeyZxOx` (hardcoded in `engine/src/config.rs` and `bet-placer/src/sportsbooks/fanduel.js`)
  - FD uses a single `eventTypeId=1` fetch for all soccer, per-page slugs for American sports
  - Fetcher: `engine/src/odds_fetcher.rs` → `fetch_fanduel()` / `fetch_fanduel_by_event_type()`

- **Pinnacle** — odds fetched directly by engine
  - Odds endpoint: `https://guest.api.arcadia.pinnacle.com/...` (per-league)
  - Auth: `x-api-key: CmX2KcMrXuFmNg6YFbmTxE0y9CIrOi0R` (hardcoded in `bet-placer/src/sportsbooks/pinnacle.js`)
  - Alternate lines filtered out via `isAlternate` flag
  - Fetcher: `engine/src/odds_fetcher.rs` → `fetch_pinnacle()`

**Sportsbook Bet Placement APIs (write — via browser automation):**

- **DraftKings** — `bet-placer/src/sportsbooks/draftkings.js`
  - Method: `page.evaluate(fetch(..., { credentials: "include" }))` — uses browser cookies
  - Calculate: `POST https://gaming-ca-on.draftkings.com/api/wager/v1/calculateBets`
  - Place: `POST https://gaming-ca-on.draftkings.com/api/wager/v1/placeBets`
  - Confirm: `GET https://gaming-ca-on.draftkings.com/api/wager/v1/getPurchases?PurchaseIds={id}`
  - Balance: `GET https://api.draftkings.com/sites/CA-ON-SB/dashes/v1/dashes/siteNav/users/me.json`
  - Auth: browser session cookies (user must be logged in manually)

- **FanDuel** — `bet-placer/src/sportsbooks/fanduel.js`
  - Method: `context.request.post()` (Node-side Playwright HTTP, bypasses PerimeterX CORS)
  - Validate: `POST https://sib.on.sportsbook.fanduel.ca/api/sports/fixedodds/transactional/v1/implyBets`
  - Place: `POST https://spb.on.sportsbook.fanduel.ca/api/sports/fixedodds/transactional/v1/placeBet`
  - Prices: `POST https://smp.on.sportsbook.fanduel.ca/api/sports/fixedodds/readonly/v1/getMarketPrices`
  - Auth: `x-authentication` + `x-auth-token` (JWT from localStorage `session-sportsbook`) + `x-geo-packet` (GeoComply JWT from `location_token`) + `x-px-context` (PerimeterX cookies `_px3`, `_pxvid`, `pxcts`)

- **Pinnacle** — `bet-placer/src/sportsbooks/pinnacle.js`
  - Method: Direct `fetch()` from Node.js process (hybrid: browser extracts tokens, Node places bet)
  - Quote: `POST https://api.arcadia.pinnacle.com/0.1/bets/straight/quote`
  - Place: `POST https://api.arcadia.pinnacle.com/0.1/bets/straight`
  - Poll: `GET https://api.arcadia.pinnacle.com/0.1/bets?requestIds={requestId}`
  - Balance: `GET https://api.arcadia.pinnacle.com/0.1/wallet/balance`
  - Auth: `x-session` + `x-device-uuid` (extracted from browser localStorage `Main:User`), `x-api-key: CmX2KcMrXuFmNg6YFbmTxE0y9CIrOi0R`
  - Token refresh: every 5 minutes (`TOKEN_REFRESH_MS`)

## Data Storage

**Databases:**
- SQLite (bundled via `rusqlite`) — single file at `engine/data/trades.db`
  - Tables: `book_balances`, `paper_trades`, `arb_opportunities`, `settings`
  - Schema in: `engine/src/db.rs` (inline `CREATE TABLE IF NOT EXISTS`)
  - Mutex-guarded single connection: `Db { conn: Mutex<Connection> }`

**File Storage:**
- Browser session profiles: `data/profiles/{bookId}/` (Chromium user-data-dir per sportsbook)
- Saved cookies: `data/cookies/{bookId}-data.json` (JSON, written on placer shutdown)
- Both directories are gitignored (contains auth state)

**Caching:**
- In-memory diff cache: `market_hashes: HashMap<String, String>` in polling loop (`engine/src/main.rs`) — prevents redundant `book.update()` calls when odds unchanged
- In-memory dedup map: `traded_keys: HashMap<String, Instant>` — 60-second cooldown per trade key

## Authentication & Identity

**DraftKings:**
- Session cookies managed by Chromium persistent context
- User must log in manually in the browser window before starting engine
- No token extraction needed — cookies flow with `credentials: "include"` fetch calls

**FanDuel:**
- JWT auth token from `localStorage["session-sportsbook"].authToken` (string or `{ jwt: "..." }`)
- Geo packet from `localStorage["location_token"].jwt` (GeoComply — requires GeoComply PLC running)
- PerimeterX tokens from HttpOnly cookies (`_px3`, `_pxvid`, `pxcts`)
- Sessions expire ~13 minutes; keepalive reloads page every 5 minutes (`KEEPALIVE_INTERVAL_MS`)

**Pinnacle:**
- Session token + device UUID from `localStorage["Main:User"]` (fields: `token`, `deviceId`, `username`)
- Tokens re-read from browser every 5 minutes
- No PerimeterX protection on Pinnacle

## Monitoring & Observability

**Error Tracking:**
- None — no external error tracking service

**Logs:**
- Structured tracing via `tracing` / `tracing-subscriber` crate (engine)
- `info` level by default, filtered via `RUST_LOG` env var or hardcoded `"info"` filter
- Node.js: plain `console.log` / `console.error` with bracket prefixes (`[DK]`, `[FD]`, `[PIN]`, `[WS]`, `[BET]`)
- No log file rotation or persistence

## CI/CD & Deployment

**Hosting:**
- Local Windows machine only (Ontario, CA region requirement)
- No containerization, no cloud deployment

**CI Pipeline:**
- None — no CI/CD configured

## Internal IPC

**Engine → Bet Placer (WebSocket, port 3402):**
- Engine connects as WS client to `ws://127.0.0.1:3402` (or `WS_URL` env var)
- Message types sent by engine: `place_bet`, `quote_bet`, `get_balances`, `ping`
- Message types received by engine: `fill_report`, `balance_update`, `balances`, `pong`
- Protocol defined in `engine/src/live_executor.rs` (Rust structs) and `bet-placer/index.js` (handlers)

**Engine REST API (HTTP, port 3401):**
- `GET /api/balances` — venue balances
- `POST /api/balances/deposit|withdraw|reset` — balance management
- `GET /api/trades` — trade history
- `GET /api/arbs` — detected arb opportunities
- `GET /api/stats` — P&L stats
- `GET|POST /api/settings` — read/update settings
- `POST /api/reset` — full data reset
- `GET /ws` — WebSocket upgrade (broadcasts live events to dashboard/monitors)

**Engine → DK Proxy (HTTP, port 3403):**
- Route: `GET /dk/{leagueId}` — proxies to DraftKings odds API
- Workaround for Rust `reqwest` TLS incompatibility with DK CDN on Windows
- Fallback: engine tries direct fetch if proxy returns empty result

## Webhooks & Callbacks

**Incoming:**
- None — no external services push data to this system

**Outgoing:**
- None — all communication is request/response or WS

## Environment Configuration

**Required for live operation:**
- `LIVE_MODE=1` — enables real bet placement
- `FIXED_STAKE` — dollar amount per leg
- `MAX_STAKE` — cap per leg
- `MIN_ARB_PCT` — minimum profit threshold (%)
- GeoComply PLC running on machine (FanDuel geo-verification)
- Chrome installed (patchright `channel: "chrome"`)
- User logged in to DraftKings, FanDuel, Pinnacle in browser windows

**Optional:**
- `STAKE_MODE`, `PROPORTIONAL_PCT`, `ENABLED`, `MAX_HOURS_TO_START`, `BALANCE_REFRESH_CYCLES`, `SKIP_LIVE`, `WS_URL`, `DK_PROXY_PORT`, `WS_PORT`

**Secrets location:**
- No `.env` file — all credentials are browser session cookies/localStorage tokens extracted at runtime
- Hardcoded API keys: FD `x-application` in `engine/src/config.rs` and `bet-placer/src/sportsbooks/fanduel.js`; PIN `x-api-key` in `bet-placer/src/sportsbooks/pinnacle.js`

---

*Integration audit: 2026-04-09*
