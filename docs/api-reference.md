# API Reference (Current State)

This reference covers the APIs the project currently uses or exposes:

- sportsbook odds APIs consumed by the Rust engine
- authenticated sportsbook betting APIs consumed by the Node bet placer
- the Rust engine REST API and dashboard WebSocket
- the private Rust-to-Node bet placement WebSocket protocol

Public app keys and hostnames listed here are already present in source code. User credentials, session tokens, cookies, and account-specific values must stay local.

## Odds APIs

### DraftKings Public Odds

Rust fetch path:

```text
GET http://localhost:3403/dk/{leagueId}
```

Proxy implementation:

```text
dk-proxy.js
```

Proxy target:

```text
GET https://sportsbook-nash.draftkings.com/sites/CA-ON-SB/api/sportscontent/dkcaon/v1/leagues/{leagueId}
```

Headers sent by the proxy:

```http
User-Agent: Mozilla/5.0 ...
Accept: application/json
```

The Rust fetcher tries the proxy first because Node `fetch` handles DraftKings' CDN/TLS behavior better on Windows. If the proxy fails or returns no markets, Rust falls back to the direct DraftKings URL.

DraftKings configured values:

```text
DK_HOSTNAME = sportsbook-nash.draftkings.com
DK_SITE_CODE = CA-ON-SB
```

### FanDuel Public Odds

Normal sports:

```text
GET https://sbapi.{state}.sportsbook.fanduel.ca/api/content-managed-page?page=CUSTOM&customPageId={page}&_ak={apiKey}
```

Soccer/event-type sports:

```text
GET https://sbapi.{state}.sportsbook.fanduel.ca/api/content-managed-page?page=SPORT&eventTypeId={eventTypeId}&_ak={apiKey}
```

Current Ontario configuration:

```text
state = on
apiKey = FhMFpcPWXMeyZxOx
soccer eventTypeId = 1
```

FanDuel public odds are parsed from the `attachments` graph and normalized into canonical markets. FanDuel adapter-ready selection IDs use:

```text
{marketId}:{selectionId}
```

### Pinnacle Authenticated Odds

Default Rust fetch path:

```text
GET http://127.0.0.1:3404/pinnacle/snapshot/{leagueId}
```

Override the local base URL with:

```text
PIN_AUTH_ODDS_BASE_URL=http://127.0.0.1:3404
```

Bet placer implementation:

```text
bet-placer/index.js
bet-placer/src/sportsbooks/pinnacle.js
```

The bet placer uses the single logged-in Pinnacle browser session, extracts `x-device-uuid` and `x-session`, then fetches:

```text
GET https://api.arcadia.pinnacle.com/0.1/leagues/{leagueId}/matchups
GET https://api.arcadia.pinnacle.com/0.1/leagues/{leagueId}/markets/straight
```

Response shape from the local bridge:

```json
{
  "status": "ok",
  "league_id": 246,
  "matchups": [],
  "markets": []
}
```

The old guest scan is still available only when explicitly requested:

```powershell
$env:PIN_ODDS_SOURCE = "guest"
```

Guest base host:

```text
https://guest.api.arcadia.pinnacle.com/0.1
```

Current Pinnacle adapter-ready selection IDs are built as:

```text
{matchupId}:{marketKey}:{designation}
```

Examples of `marketKey` shape:

```text
s;0;m
s;0;s;-1.5
s;0;ou;5.5
s;0;tt;3.5;home
```

## Configured Sports

Configured in `engine/src/config.rs`.

| Sport | DraftKings | FanDuel | Pinnacle |
| --- | ---: | --- | --- |
| NBA | 42648 | `nba` | 487 |
| NHL | 42133 | `nhl` | 1456 |
| MLB | 84240 | `mlb` | 246 |
| NFL | 88808 | `nfl` | none currently enabled |
| NCAAB | 92483 | `ncaab` | 493 |
| NCAAF | 87637 | `ncaaf` | 880 |
| Soccer root | none | event type 1 | none |
| EPL | 40253 | soccer event type | 1980 |
| La Liga | 40031 | soccer event type | 2196 |
| Serie A | 40030 | soccer event type | 2436 |
| Bundesliga | 40481 | soccer event type | 1842 |
| Ligue 1 | 40032 | soccer event type | 2036 |
| UCL | 40685 | soccer event type | 2627 |
| MLS | 89345 | soccer event type | 2663 |
| UFC | 9034 | `ufc` | 1624 |

NFL has no current Pinnacle league ID in the live config because the attempted Pinnacle league path returned 401 during earlier guest-source testing.

## Authenticated Betting APIs

The Node bet placer owns authenticated sportsbook calls. It launches Patchright persistent Chrome contexts and uses the logged-in browser sessions.

### DraftKings Betting

Adapter:

```text
bet-placer/src/sportsbooks/draftkings.js
```

Transport:

```text
page.evaluate(fetch(..., credentials: "include"))
```

This means DraftKings wager calls run inside the authenticated DraftKings browser page and use browser cookies.

Hosts:

```text
gaming: https://gaming-ca-on.draftkings.com
content: https://sportsbook-nash.draftkings.com
api: https://api.draftkings.com
origin: https://sportsbook.draftkings.com
```

Main flow:

1. `GET /api/sdinfo/dkcaon/v1/selectioninfo?siteName=dkcaon&selectionIds={selectionId}`
2. `POST /api/wager/v1/calculateBets`
3. `POST /api/wager/v1/placeBets`
4. `GET /api/wager/v1/getPurchases?PurchaseIds={waitingBetId}&format=json`

Standard wager headers currently sent:

```http
clienttype: Website
content-type: application/json
x-api-features: {"EnableFullSGPDrivenFlow":true}
x-client-feature: betslip
x-client-name: web
x-client-version: 2612.4.1.6
x-client-widget-name: betslip
x-client-widget-version: 2608.3.1
```

Placement adds:

```http
calculationid: {calculationId}
x-request-client-timestamp: {Date.now()}
```

Balance endpoint:

```text
GET https://api.draftkings.com/sites/CA-ON-SB/dashes/v1/dashes/siteNav/users/me.json?format=json
```

The adapter filters out `XPP` balances and returns cash accounts.

### FanDuel Betting

Adapter:

```text
bet-placer/src/sportsbooks/fanduel.js
```

Transport:

```text
context.request.get/post(...)
```

FanDuel uses Patchright's Node-side request API with cookies from the browser context plus extracted localStorage/cookie tokens for normal market/placement calls.

Hosts:

```text
imply: https://sib.on.sportsbook.fanduel.ca
place: https://spb.on.sportsbook.fanduel.ca
prices: https://smp.on.sportsbook.fanduel.ca
account: https://api.fanduel.ca
origin: https://on.sportsbook.fanduel.ca
```

Headers are built dynamically:

```http
content-type: application/json
accept: application/json
x-application: FhMFpcPWXMeyZxOx
origin: https://on.sportsbook.fanduel.ca
referer: https://on.sportsbook.fanduel.ca/
x-authentication: {auth token, if found}
x-auth-token: {auth token, if found}
x-px-context: {_px3/_pxvid/pxcts cookie context, if found}
x-geo-packet: {geolocation_document.jwt, falling back to location_token.jwt}
```

Main flow:

1. `POST /api/sports/fixedodds/readonly/v1/getMarketPrices?priceHistory=1`
2. `POST /api/sports/fixedodds/transactional/v1/implyBets?pricePolicy=SUGGESTED`
3. `POST /api/sports/fixedodds/transactional/v1/placeBet`

FanDuel selection IDs use:

```text
{marketId}:{selectionId}
```

FanDuel balance is currently scraped from the visible page. The adapter reads the username from session JWT data when available and chooses the largest visible dollar amount from simple page elements. This is fragile and should be treated as operationally sensitive.

FanDuel status/cashout flow:

```text
GET https://fcq.on.sportsbook.fanduel.ca/api/v0.1/getEligibleBets?sortBy=START_TIME
POST https://sco.on.sportsbook.fanduel.ca/api/sports/fixedodds/transactional/v1.0/cashoutBet
```

`getEligibleBets` returns open bets plus fresh `cashOutToken` values. `cashoutBet` is sent from inside the logged-in FanDuel browser page, not through the older Node-side request path, so the write can refresh and use the current `x-geo-packet` JWT.

### Pinnacle Betting

Adapter:

```text
bet-placer/src/sportsbooks/pinnacle.js
```

Transport:

```text
direct Node fetch
```

Base:

```text
https://api.arcadia.pinnacle.com/0.1
```

Static application key in source:

```text
x-api-key: CmX2KcMrXuFmNg6YFbmTxE0y9CIrOi0R
```

Runtime auth is extracted from the Pinnacle browser page. The adapter searches common localStorage/sessionStorage objects first, including `Main:User`, then falls back to a localStorage scan:

```text
localStorage["Main:User"].deviceId -> x-device-uuid
localStorage["Main:User"].token    -> x-session
localStorage["Main:User"].username -> display username
```

Headers:

```http
content-type: application/json
accept: application/json
x-api-key: {static app key}
origin: https://www.pinnacle.ca
referer: https://www.pinnacle.ca/
x-device-uuid: {device id, if found}
x-session: {session token, if found}
```

Main flow:

1. `POST /bets/straight/quote`
2. `POST /bets/straight`
3. `GET /bets?requestIds={requestId}` until accepted/non-pending or timeout

Selection info:

```text
GET /matchups/{matchupId}/market/{marketKey}
```

Balance:

```text
GET /wallet/balance
```

Pinnacle quote responses also supply min/max stake limits used by live execution before any leg is placed.

## Rust Engine REST API

Base URL:

```text
http://localhost:3401
```

Important security note: the engine binds to `0.0.0.0:3401`, allows permissive CORS, and has no authentication. Treat it as a trusted-local API only.

### `GET /api/balances`

Returns all stored book balances.

Response:

```json
[
  {
    "venue": "draftkings",
    "balance": 1000.0,
    "initial_balance": 1000.0,
    "updated_at": "2026-05-03T12:00:00Z"
  }
]
```

### `POST /api/balances/deposit`

Adds to a venue balance.

Request:

```json
{
  "venue": "draftkings",
  "amount": 25.0
}
```

Response: same shape as `GET /api/balances`.

### `POST /api/balances/withdraw`

Subtracts from a venue balance, clamped at zero.

Request:

```json
{
  "venue": "fanduel",
  "amount": 10.0
}
```

Response: same shape as `GET /api/balances`.

### `POST /api/balances/reset`

Resets current balances to `initial_balance`.

Response: same shape as `GET /api/balances`.

### `GET /api/trades`

Returns up to 200 recent trades ordered by `executed_at DESC`.

Response:

```json
[
  {
    "id": "uuid",
    "detected_at": "2026-05-03T12:00:00Z",
    "executed_at": "2026-05-03T12:00:01Z",
    "event_id": "NBA:2026-03-15T19:BOS-vs-GSW",
    "sport": "NBA",
    "market_type": "moneyline",
    "leg1_venue": "draftkings",
    "leg1_selection": "BOS",
    "leg1_odds": 2.1,
    "leg1_stake": 24.39,
    "leg2_venue": "pinnacle",
    "leg2_selection": "GSW",
    "leg2_odds": 2.05,
    "leg2_stake": 25.61,
    "sigma": 0.9639,
    "theoretical_profit": 1.87,
    "status": "executed"
  }
]
```

Live trades use status `executed_live`.

### `GET /api/arbs`

Returns up to 200 recent detected opportunities ordered by `detected_at DESC`.

Response:

```json
[
  {
    "id": "uuid",
    "detected_at": "2026-05-03T12:00:00Z",
    "event_id": "NBA:2026-03-15T19:BOS-vs-GSW",
    "sport": "NBA",
    "market_type": "moneyline",
    "sigma": 0.9639,
    "arb_pct": 0.0361,
    "leg1_venue": "draftkings",
    "leg1_selection": "BOS",
    "leg1_odds": 2.1,
    "leg2_venue": "pinnacle",
    "leg2_selection": "GSW",
    "leg2_odds": 2.05,
    "traded": false
  }
]
```

### `GET /api/stats`

Returns aggregate dashboard stats from SQLite.

Shape is defined by `models::Stats`.

### `GET /api/settings`

Returns execution settings.

Response:

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

### `POST /api/settings`

Replaces current settings with the supplied settings object and broadcasts `settings_updated`.

Request:

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

Response: the saved settings object.

### `POST /api/reset`

Clears runtime data through `db.full_reset()` and broadcasts updated balances.

Response:

```json
{
  "status": "reset complete"
}
```

## Dashboard WebSocket

Endpoint:

```text
ws://localhost:3401/ws
```

The dashboard WebSocket is broadcast-only from the engine to connected clients.

Event types:

### `arb_detected`

```json
{
  "type": "arb_detected",
  "arb": {}
}
```

`arb` has the same shape as a row from `GET /api/arbs`.

### `trade_executed`

```json
{
  "type": "trade_executed",
  "trade": {}
}
```

`trade` has the same shape as a row from `GET /api/trades`.

### `balance_updated`

```json
{
  "type": "balance_updated",
  "balances": []
}
```

`balances` has the same shape as `GET /api/balances`.

### `settings_updated`

```json
{
  "type": "settings_updated",
  "settings": {}
}
```

### `poll_status`

```json
{
  "type": "poll_status",
  "venue": "draftkings",
  "markets": 123
}
```

### `error`

```json
{
  "type": "error",
  "message": "details"
}
```

## Engine-to-Bet-Placer WebSocket

Endpoint:

```text
ws://localhost:3402
```

Override with:

```text
WS_URL=ws://127.0.0.1:3402
WS_PORT=3402
```

This protocol is private to the Rust engine and Node bet placer.

### `get_balances`

Request:

```json
{
  "type": "get_balances"
}
```

Response:

```json
{
  "type": "balances",
  "balances": [
    { "venue": "draftkings", "balance": 1000.0 },
    { "venue": "fanduel", "balance": 1000.0 },
    { "venue": "pinnacle", "balance": 1000.0 }
  ]
}
```

### `quote_bet`

Request:

```json
{
  "type": "quote_bet",
  "attempt_id": "uuid-Q",
  "venue": "pinnacle",
  "venue_market_id": "123456:s;0;m:home"
}
```

Success response:

```json
{
  "type": "quote_result",
  "attempt_id": "uuid-Q",
  "status": "ok",
  "min_stake": 1.0,
  "max_stake": 250.0,
  "price": 2.05
}
```

Error response:

```json
{
  "type": "quote_result",
  "attempt_id": "uuid-Q",
  "status": "error",
  "error_message": "quote failed"
}
```

Non-Pinnacle venues currently return a synthetic ok quote with `min_stake=0`, `max_stake=999999`, and `price=0`. Live execution only relies on quote details for Pinnacle.

### `validate_soft`

Request:

```json
{
  "type": "validate_soft",
  "attempt_id": "uuid-VS",
  "venue": "draftkings",
  "venue_market_id": "0ML84034728_3",
  "stake": 5.0,
  "expected_odds": 4.5,
  "max_slippage_pct": 0.07
}
```

Success response:

```json
{
  "type": "validate_soft_result",
  "attempt_id": "uuid-VS",
  "status": "ok",
  "price": 4.5,
  "display_odds": "+350"
}
```

Error response:

```json
{
  "type": "validate_soft_result",
  "attempt_id": "uuid-VS",
  "status": "error",
  "error_message": "validate_soft returned no executable price",
  "error_code": null
}
```

DraftKings validation uses `calculateBets`. FanDuel validation uses `getSelectionInfo`, which refreshes market prices via the logged-in FanDuel adapter.

### `place_bet`

Request:

```json
{
  "type": "place_bet",
  "attempt_id": "uuid-L1",
  "venue": "draftkings",
  "venue_market_id": "0ML84034728_3",
  "selection": "BOS",
  "stake": 25.0,
  "expected_odds": 2.1
}
```

Filled response:

```json
{
  "type": "fill_report",
  "attempt_id": "uuid-L1",
  "status": "filled",
  "accepted_stake": 25.0,
  "accepted_odds": 2.1,
  "purchase_id": "123456789",
  "bet_id": "123456790",
  "error_message": null
}
```

Rejected response:

```json
{
  "type": "fill_report",
  "attempt_id": "uuid-L1",
  "status": "rejected",
  "accepted_stake": null,
  "accepted_odds": null,
  "error_message": "placeBet failed"
}
```

After a filled bet, Node best-effort sends:

```json
{
  "type": "balance_update",
  "venue": "draftkings",
  "balance": 975.0
}
```

### `get_bet_status`

Request:

```json
{
  "type": "get_bet_status",
  "attempt_id": "uuid-STATUS",
  "venue": "draftkings",
  "purchase_id": "123456789",
  "bet_id": "123456790"
}
```

Response:

```json
{
  "type": "bet_status",
  "attempt_id": "uuid-STATUS",
  "venue": "draftkings",
  "purchase_id": "123456789",
  "bet_id": "123456790",
  "status": "open",
  "raw_status": "Unsettled",
  "cashout_available": true,
  "cashout_offer": 4.83
}
```

DraftKings status is based on `getPurchases`. FanDuel status is based on `getEligibleBets`; if the bet is no longer in open bets, the adapter returns `unknown` with raw status `Not Found In Open Bets`.

### `cashout`

Request:

```json
{
  "type": "cashout",
  "attempt_id": "uuid-CASHOUT",
  "venue": "fanduel",
  "purchase_id": "987654321",
  "bet_id": "987654321",
  "amount": 4.83
}
```

Response:

```json
{
  "type": "cashout_result",
  "attempt_id": "uuid-CASHOUT",
  "venue": "fanduel",
  "bet_id": "987654321",
  "success": true,
  "confirmed": true,
  "unconfirmed": false,
  "submitted": false,
  "terminal_non_cashout": false,
  "payout": 4.83,
  "payout_claimed": null,
  "raw_status": "Cashed Out",
  "reason": null,
  "error": null
}
```

DraftKings cashout requires raw `Cashed Out` confirmation or a terminal non-cashout classification. FanDuel cashout fetches a fresh `cashOutToken` before sending `cashoutBet`.

### `ping`

Request:

```json
{
  "type": "ping"
}
```

Response:

```json
{
  "type": "pong"
}
```

## Environment Variables

Rust engine:

| Variable | Meaning | Default |
| --- | --- | --- |
| `LIVE_MODE` | Connect to bet placer and attempt live execution when `1`/`true` | false |
| `FIXED_STAKE` | Startup override for settings fixed stake | DB/default |
| `MAX_STAKE` | Startup override for max non-Pinnacle soft-book stake; Pinnacle hedge is quote/balance bounded | DB/default |
| `MIN_ARB_PCT` | Startup override for minimum arb percent | DB/default |
| `STAKE_MODE` | `fixed` or `proportional` | DB/default |
| `PROPORTIONAL_PCT` | Startup override for proportional staking | DB/default |
| `ENABLED` | Startup override for execution toggle | DB/default |
| `MAX_HOURS_TO_START` | Pregame capital lockup filter | 24 |
| `BALANCE_REFRESH_CYCLES` | Live balance refresh interval in poll cycles | 60 |
| `NAKED_LEG_MONITOR_CYCLES` | Recovery monitor interval in poll cycles | 10 |
| `SKIP_LIVE` | Skip live in-play markets when `1`/`true` | false |
| `ODDS_FETCH_TIMEOUT_MS` | Per-request odds fetch deadline for DK/FD/PIN scanning | 2500 |
| `WS_URL` | Bet placer WebSocket URL | `ws://127.0.0.1:3402` |
| `PIN_ODDS_SOURCE` | `auth`/`authenticated`/`logged-in` for logged-in PIN snapshot, or `guest`/`public` for old guest API | `auth` |
| `PIN_AUTH_ODDS_BASE_URL` | Local bet-placer PIN odds bridge base URL | `http://127.0.0.1:3404` |
| `QUOTE_TIMEOUT_MS` | General quote timeout | 10000 |
| `PIN_FILL_TIMEOUT_MS` | Pinnacle fill-report wait timeout | 45000 |
| `FILL_TIMEOUT_MS` | Non-Pinnacle fill-report wait timeout | 20000 |
| `SOFT_PREFIRE_VALIDATION_ENABLED` | Enable DK/FD authenticated soft validation | true |
| `SOFT_PREFIRE_VALIDATION_TIMEOUT_MS` | DK/FD validation timeout | 3500 |
| `SOFT_PREFIRE_MAX_SLIPPAGE_PCT` | Sent to the bet placer with validation requests | 0.07 |
| `PIN_PREFIRE_RECHECK_ENABLED` | Enable final Pinnacle quote before soft-leg risk | true |
| `PIN_PREFIRE_RECHECK_TIMEOUT_MS` | Final Pinnacle quote timeout | 1500 |
| `PIN_YANK_BLACKLIST_MS` | TTL for hard-yanked Pinnacle market IDs | 1800000 |
| `RESCUE_PIN_HARD_YANK_THRESHOLD` | Rescue hard-yank miss threshold before stopping PIN rescue attempts | 3 |
| `RESCUE_QUOTE_TIMEOUT_MS` | Pinnacle rescue quote timeout | 3000 |
| `MAX_RESCUE_LOSS` | Optional max allowed rescue loss; unset means no explicit cap | unset |
| `BET_STATUS_TIMEOUT_MS` | `get_bet_status` response timeout | 5000 |
| `CASHOUT_TIMEOUT_MS` | `cashout` response timeout | 15000 |
| `NAKED_LEG_PIN_RECHECK_QUOTE_TIMEOUT_MS` | Monitor quote timeout for relisted PIN rescue | 3000 |
| `ATTEMPT_TELEMETRY_DIR` | Override telemetry root | `data` |
| `NAKED_LEGS_FILE` | Override naked-leg ledger file | `data/naked-legs.json` |

Node:

| Variable | Meaning | Default |
| --- | --- | --- |
| `WS_PORT` | Bet placer WebSocket port | 3402 |
| `PIN_ODDS_PORT` | Bet placer local HTTP port for authenticated PIN snapshots | 3404 |
| `DK_PROXY_PORT` | DraftKings proxy port | 3403 |
| `DK_PROXY_FETCH_TIMEOUT_MS` | DraftKings proxy upstream fetch deadline; falls back to `ODDS_FETCH_TIMEOUT_MS` if set | 2500 |
| `FD_WRITE_TIMEOUT_MS` | FanDuel browser-page write timeout for cashout | 25000 |

## Compatibility Notes

- The Rust engine uses `danger_accept_invalid_certs(true)` for its odds HTTP client because of Windows/DraftKings TLS issues.
- The DraftKings odds proxy is local-only HTTP and returns permissive CORS.
- `engine/config.toml` contains older policy values and is not authoritative for the current main runtime.
- The REST API and dashboard WebSocket are unauthenticated.
