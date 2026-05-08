# API Reference

This document covers the APIs currently exposed or consumed by the project:

- Rust REST API on `3401`;
- dashboard WebSocket on `3401`;
- private Rust-to-bet-placer WebSocket on `3402`;
- local DraftKings odds proxy on `3403`;
- local authenticated Pinnacle odds bridge on `3404`;
- sportsbook endpoints used by the adapters.

Credentials, cookies, session tokens, geo tokens, and account-specific values must stay local.

## Local Services

| Service | Default URL | Owner |
| --- | --- | --- |
| Rust REST API | `http://localhost:3401` | `engine/src/main.rs` |
| Dashboard WebSocket | `ws://localhost:3401/ws` | `engine/src/api.rs` |
| Bet placer WebSocket | `ws://localhost:3402` | `bet-placer/index.js` |
| DraftKings odds proxy | `http://localhost:3403` | `dk-proxy.js` |
| Pinnacle auth odds bridge | `http://localhost:3404` | `bet-placer/index.js` |

The Rust REST API binds to `0.0.0.0:3401`, allows permissive CORS, and has no auth.

## Rust REST API

### `GET /api/balances`

Returns stored balances.

```json
[
  {
    "venue": "draftkings",
    "balance": 1000.0,
    "initial_balance": 1000.0,
    "updated_at": "2026-05-08T12:00:00Z"
  }
]
```

### `POST /api/balances/deposit`

Adds to a venue balance.

```json
{
  "venue": "draftkings",
  "amount": 25.0
}
```

Response is the full balance list.

### `POST /api/balances/withdraw`

Subtracts from a venue balance and clamps at zero.

```json
{
  "venue": "fanduel",
  "amount": 10.0
}
```

Response is the full balance list.

### `POST /api/balances/reset`

Resets balances to `initial_balance`.

Response is the full balance list.

### `GET /api/trades`

Returns up to 200 recent trades.

Live trades use status `executed_live`. Paper trades use status `executed`.

### `GET /api/arbs`

Returns up to 200 recent detected arb opportunities.

### `GET /api/stats`

Returns aggregate stats from SQLite.

### `GET /api/settings`

Returns active execution settings.

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

Replaces active settings and broadcasts `settings_updated`.

### `POST /api/reset`

Runs a full runtime reset through `db.full_reset()` and broadcasts updated balances.

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

Broadcast event types:

- `arb_detected`
- `trade_executed`
- `balance_updated`
- `settings_updated`
- `poll_status`
- `error`

This WebSocket is separate from the private bet-placement WebSocket.

## Bet Placer WebSocket

Endpoint:

```text
ws://localhost:3402
```

Override with:

```text
WS_URL=ws://127.0.0.1:3402
WS_PORT=3402
```

This is a private local protocol between Rust and Node.

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
    { "venue": "draftkings", "balance": 100.0 },
    { "venue": "fanduel", "balance": 100.0 },
    { "venue": "pinnacle", "balance": 100.0 }
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
  "venue_market_id": "1630254037:s;0;m:home"
}
```

Success:

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

Error:

```json
{
  "type": "quote_result",
  "attempt_id": "uuid-Q",
  "status": "error",
  "error_message": "quote failed"
}
```

Live execution relies on executable quote fields only for Pinnacle.

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

Success:

```json
{
  "type": "validate_soft_result",
  "attempt_id": "uuid-VS",
  "status": "ok",
  "price": 4.5,
  "display_odds": "+350"
}
```

Error:

```json
{
  "type": "validate_soft_result",
  "attempt_id": "uuid-VS",
  "status": "error",
  "error_message": "validate_soft returned no executable price",
  "error_code": null
}
```

DraftKings validation uses `calculateBets`. FanDuel validation uses authenticated price refresh.

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
  "purchase_id": null,
  "bet_id": null,
  "error_message": "placeBet failed"
}
```

ID handling:

- Node sends `purchase_id` and `bet_id` as strings.
- Rust also accepts numeric IDs so Pinnacle accepted responses cannot be dropped because of JSON type mismatch.
- `unknown` is not a valid placement status for live automation.

After a filled bet, Node may also send:

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

Status may be `unknown` when a status lookup cannot locate or classify the bet. That is separate from placement fill status and must not be treated as confirmed placement.

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

DraftKings cashout requires an available offer. FanDuel cashout fetches a fresh `cashOutToken` before writing.

### `ping`

Request:

```json
{ "type": "ping" }
```

Response:

```json
{ "type": "pong" }
```

## Odds APIs

### DraftKings Public Odds

Rust proxy URL:

```text
GET http://localhost:3403/dk/{leagueId}
```

Proxy target:

```text
GET https://sportsbook-nash.draftkings.com/sites/CA-ON-SB/api/sportscontent/dkcaon/v1/leagues/{leagueId}
```

The proxy deadline is `DK_PROXY_FETCH_TIMEOUT_MS`, or `ODDS_FETCH_TIMEOUT_MS`, default `2500`.

### FanDuel Public Odds

Normal sports:

```text
GET https://sbapi.on.sportsbook.fanduel.ca/api/content-managed-page?page=CUSTOM&customPageId={page}&_ak=FhMFpcPWXMeyZxOx
```

Soccer:

```text
GET https://sbapi.on.sportsbook.fanduel.ca/api/content-managed-page?page=SPORT&eventTypeId=1&_ak=FhMFpcPWXMeyZxOx
```

### Pinnacle Authenticated Odds

Rust local bridge:

```text
GET http://127.0.0.1:3404/pinnacle/snapshot/{leagueId}
```

Bridge response:

```json
{
  "status": "ok",
  "league_id": 246,
  "matchups": [],
  "markets": []
}
```

The bet placer fetches:

```text
GET https://api.arcadia.pinnacle.com/0.1/leagues/{leagueId}/matchups
GET https://api.arcadia.pinnacle.com/0.1/leagues/{leagueId}/markets/straight
```

Guest mode is opt-in:

```powershell
$env:PIN_ODDS_SOURCE = "guest"
```

## Configured Sports

Configured in `engine/src/config.rs`.

| Sport | DraftKings ID | FanDuel source | Pinnacle league IDs |
| --- | ---: | --- | --- |
| NBA | 42648 | `nba` | 487 |
| NHL | 42133 | `nhl` | 1456 |
| MLB | 84240 | `mlb` | 246 |
| NFL | 88808 | `nfl` | none |
| NCAAB | 92483 | `ncaab` | 493 |
| NCAAF | 87637 | `ncaaf` | 880 |
| SOCCER | none | event type `1` | none |
| EPL | 40253 | included in SOCCER fetch | 1980 |
| LA_LIGA | 40031 | included in SOCCER fetch | 2196 |
| SERIE_A | 40030 | included in SOCCER fetch | 2436 |
| BUNDESLIGA | 40481 | included in SOCCER fetch | 1842 |
| LIGUE_1 | 40032 | included in SOCCER fetch | 2036 |
| UCL | 40685 | included in SOCCER fetch | 2627 |
| MLS | 89345 | included in SOCCER fetch | 2663 |
| UFC | 9034 | none | 1624 |

## Adapter Betting APIs

DraftKings adapter:

- `selectioninfo`;
- `calculateBets`;
- `placeBets`;
- `getPurchases`;
- `cashout`.

FanDuel adapter:

- `getMarketPrices`;
- `implyBets`;
- `placeBet`;
- `getEligibleBets`;
- browser-context `cashoutBet`.

Pinnacle adapter:

- `/bets/straight/quote`;
- `/bets/straight`;
- `/bets?requestIds={requestId}`;
- `/wallet/balance`;
- authenticated league snapshot endpoints.

## Environment Variables

Rust:

| Variable | Meaning | Default |
| --- | --- | --- |
| `LIVE_MODE` | Enable live WebSocket execution when `1` | false |
| `FIXED_STAKE` | Fixed-mode requested soft-book stake | SQLite/default |
| `MAX_STAKE` | Non-Pinnacle soft-book cap | SQLite/default |
| `MIN_ARB_PCT` | Minimum arb percent | SQLite/default |
| `STAKE_MODE` | `fixed` or `proportional` | SQLite/default |
| `PROPORTIONAL_PCT` | Proportional stake percent | SQLite/default |
| `ENABLED` | Execution toggle | SQLite/default |
| `MAX_HOURS_TO_START` | Pregame event horizon | 24 |
| `SKIP_LIVE` | Skip live markets | false |
| `BALANCE_REFRESH_CYCLES` | Balance refresh interval in poll cycles | 60 |
| `NAKED_LEG_MONITOR_CYCLES` | Naked-leg monitor interval in poll cycles | 10 |
| `ODDS_FETCH_TIMEOUT_MS` | Per-odds-request deadline | 2500 |
| `WS_URL` | Bet placer WebSocket URL | `ws://127.0.0.1:3402` |
| `PIN_ODDS_SOURCE` | `auth`, `authenticated`, `logged-in`, `logged_in`, `guest`, or `public` | `auth` |
| `PIN_AUTH_ODDS_BASE_URL` | Pinnacle auth odds bridge | `http://127.0.0.1:3404` |
| `QUOTE_TIMEOUT_MS` | General quote timeout | 10000 |
| `PIN_FILL_TIMEOUT_MS` | Pinnacle fill timeout | 45000 |
| `FILL_TIMEOUT_MS` | Non-Pinnacle fill timeout | 20000 |
| `SOFT_PREFIRE_VALIDATION_ENABLED` | Enable DK/FD validation | true |
| `SOFT_PREFIRE_VALIDATION_TIMEOUT_MS` | DK/FD validation timeout | 3500 |
| `SOFT_PREFIRE_MAX_SLIPPAGE_PCT` | Sent with validation command | 0.07 |
| `PIN_PREFIRE_RECHECK_ENABLED` | Enable final PIN recheck | true |
| `PIN_PREFIRE_RECHECK_TIMEOUT_MS` | Final PIN recheck timeout | 1500 |
| `PIN_YANK_BLACKLIST_MS` | Exact-market hard-yank TTL | 1800000 |
| `RESCUE_PIN_HARD_YANK_THRESHOLD` | Rescue hard-yank threshold | 3 |
| `RESCUE_QUOTE_TIMEOUT_MS` | Rescue quote timeout | 3000 |
| `MAX_RESCUE_LOSS` | Optional rescue-loss cap | unset |
| `BET_STATUS_TIMEOUT_MS` | Bet status timeout | 5000 |
| `CASHOUT_TIMEOUT_MS` | Cashout response timeout | 15000 |
| `NAKED_LEG_PIN_RECHECK_QUOTE_TIMEOUT_MS` | Monitor relist quote timeout | 3000 |
| `ATTEMPT_TELEMETRY_DIR` | Telemetry root | `data` |
| `NAKED_LEGS_FILE` | Naked-leg ledger | `data/naked-legs.json` |

Node:

| Variable | Meaning | Default |
| --- | --- | --- |
| `WS_PORT` | Bet placer WebSocket port | 3402 |
| `PIN_ODDS_PORT` | Pinnacle auth odds bridge port | 3404 |
| `DK_PROXY_PORT` | DraftKings proxy port | 3403 |
| `DK_PROXY_FETCH_TIMEOUT_MS` | DK proxy upstream deadline | 2500 |
| `FD_WRITE_TIMEOUT_MS` | FD browser-page cashout write timeout | 25000 |
