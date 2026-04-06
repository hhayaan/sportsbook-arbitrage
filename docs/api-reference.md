# API Reference

## Odds APIs (Public, No Auth)

Called by the Rust engine every 5 seconds. No browser or authentication needed.

### DraftKings Odds

```
GET https://sportsbook-nash.draftkings.com/sites/CA-ON-SB/api/sportscontent/dkcaon/v1/leagues/{leagueId}
```

Fetched via dk-proxy.js on `http://localhost:3403/dk/{leagueId}` due to Rust TLS issue on Windows.

Response: flat arrays `events[]`, `markets[]`, `selections[]` joined by `eventId` and `marketId`.

League IDs: NBA=42648, NHL=42133, MLB=84240, NFL=88808, NCAAB=92483, NCAAF=87637, EPL=40253, La Liga=40031, Serie A=40030, Bundesliga=40481, Ligue 1=40032, UCL=40685, MLS=89345, UFC=9034

### FanDuel Odds

```
GET https://sbapi.on.sportsbook.fanduel.ca/api/content-managed-page?page=CUSTOM&customPageId={sport}&_ak=FhMFpcPWXMeyZxOx
```

Response: `attachments: { events: {id: event}, markets: {id: market} }` as objects keyed by ID. Markets contain `runners[]` with odds.

Sport pages: `nba`, `nhl`, `mlb`, `nfl`, `ncaab`, `ncaaf`. Soccer uses `eventType=1` via a separate endpoint.

### Pinnacle Odds

```
GET https://guest.api.arcadia.pinnacle.com/0.1/leagues/{leagueId}/matchups
GET https://guest.api.arcadia.pinnacle.com/0.1/leagues/{leagueId}/markets/straight
```

No auth needed (guest API). Returns matchups with participants, and markets with American odds prices.

League IDs: NBA=487, NHL=1456, MLB=246, NCAAB=493, NCAAF=880, EPL=1980, La Liga=2196, Serie A=2436, Bundesliga=1842, Ligue 1=2036, UCL=2627, MLS=2663, UFC=1624

---

## DraftKings Betting API

All calls via `page.evaluate(fetch(...))` with `credentials: "include"` to send HttpOnly cookies.

### Headers

```json
{
  "clienttype": "Website",
  "content-type": "application/json",
  "x-client-name": "web",
  "x-client-version": "2612.4.1.6",
  "x-client-feature": "betslip",
  "x-client-widget-name": "betslip"
}
```

### Step 1: Get Selection Info

```
GET https://sportsbook-nash.draftkings.com/api/sdinfo/dkcaon/v1/selectioninfo?siteName=dkcaon&selectionIds={selId}
```

### Step 2: Calculate Bets

```
POST https://gaming-ca-on.draftkings.com/api/wager/v1/calculateBets

Body: { selections: [{ id: "0ML84036019_3" }], selectionsForYourBet: [], oddsStyle: "american" }
```

Response body: `{ bets: [{ token }], selections: [{ trueOdds, displayOdds, points? }] }`
`points` is returned for spread/total markets (e.g., `1.5`, `226.5`). Not present for moneylines.
Response header: `calculationid` (required for placement)

### Step 3: Place Bets

```
POST https://gaming-ca-on.draftkings.com/api/wager/v1/placeBets
Headers: { calculationid: "<from step 2>" }

Body: {
  bets: [{ type: "Single", selectionsMapped: [{ id }], stake, trueOdds, displayOdds, potentialReturns, token }],
  selections: [{ id, trueOdds, displayOdds, points? }],
  autoAcceptMode: "AcceptHigher"
}
`points` must be included in `selections` for spread/total bets. Without it, DK returns `NewOffer` and silently refunds. Moneylines omit `points`.
```

### Step 4: Poll (if needed)

```
GET https://gaming-ca-on.draftkings.com/api/wager/v1/getPurchases?PurchaseIds={waitingBetId}&format=json
```

Poll every 1s up to 15s. Status: `LiveDelay` -> `Open` (confirmed). `NewOffer` means odds changed and bet was not accepted (will be refunded).

### Selection ID Format

`0ML{marketId}_{outcomeIndex}` (moneyline), `0HC{marketId}...` (spread), `0OU{marketId}...` (total)

---

## FanDuel Betting API

All calls via `context.request.post()` (Node-side HTTP, bypasses PerimeterX CORS).

### Headers

| Header | Source |
|--------|--------|
| `x-authentication` | `localStorage["session-sportsbook"]` -> `authToken.jwt` |
| `x-geo-packet` | `localStorage["location_token"]` -> `jwt` |
| `x-px-context` | HttpOnly cookies `_px3`, `_pxvid`, `pxcts` via `context.cookies()` |
| `x-application` | Hardcoded `FhMFpcPWXMeyZxOx` |

### Step 1: Get Market Prices

```
POST https://smp.on.sportsbook.fanduel.ca/api/sports/fixedodds/readonly/v1/getMarketPrices?priceHistory=1

Body: { marketIds: ["801.161172464"] }
```

### Step 2: Imply Bets (Validate)

```
POST https://sib.on.sportsbook.fanduel.ca/api/sports/fixedodds/transactional/v1/implyBets?pricePolicy=SUGGESTED

Body: { betLegs: [{ legType: "SIMPLE_SELECTION", betRunners: [{ runner: { marketId, selectionId, handicap: 0 } }] }] }
```

Response: `{ respCode: "SUCCESS", betCombinations: [{ betReference, winAvgOdds }] }`

### Step 3: Place Bet

```
POST https://spb.on.sportsbook.fanduel.ca/api/sports/fixedodds/transactional/v1/placeBet

Body: {
  customerRef: <uuid>, betDefinitions: [{
    betReference, betType: "SINGLE", stakePerLine: <stake>,
    winExpectedOdds: { trueOdds: { decimalOdds: { decimalOdds } } },
    legs: [{ legType: "SIMPLE_SELECTION", betRunners: [{ runner: { marketId, selectionId, handicap } }],
      winExpectedOdds: { fractionalOdds: { numerator, denominator } } }]
  }]
}
```

### Selection ID Format

`{marketId}:{selectionId}` (e.g., `801.161172464:60427`)

---

## Pinnacle Betting API

All calls via `context.request.post()` with header-based auth.

### Headers

| Header | Source |
|--------|--------|
| `x-api-key` | Static: `CmX2KcMrXuFmNg6YFbmTxE0y9CIrOi0R` |
| `x-device-uuid` | `localStorage["Main:User"]` -> `deviceId` |
| `x-session` | `localStorage["Main:User"]` -> `token` |

### Step 1: Quote (Validate)

```
POST https://api.arcadia.pinnacle.com/0.1/bets/straight/quote

Body: { oddsFormat: "american", selections: [{ matchupId, marketKey, designation, price: 0 }] }
```

Response: `{ selections: [{ marketId, price, designation, marketKey, matchupId }], limits: [...] }`

The `marketId` from the quote is required for placement.

### Step 2: Place Bet

```
POST https://api.arcadia.pinnacle.com/0.1/bets/straight

Body: {
  oddsFormat: "american", requestId: <uuid>, acceptBetterPrice: true,
  class: "Straight", stake: <amount>,
  selections: [{ marketId, matchupId, marketKey, designation, price }],
  originTag: "sl:bsd"
}
```

Response: `{ requestId, status: "PENDING_ACCEPTANCE" }`

### Step 3: Poll for Acceptance

```
GET https://api.arcadia.pinnacle.com/0.1/bets?requestIds={requestId}
```

Poll every 1.5s up to 20s. Response transitions from `{ status: "pending" }` to full bet object with `id`, `stake`, `toWin`, `price`.

### Step 4: Balance

```
GET https://api.arcadia.pinnacle.com/0.1/wallet/balance

Response: { amount: 3932.00, currency: "CAD" }
```

### Selection ID Format

`{matchupId}:{marketKey}:{designation}` (e.g., `1627197304:s;0;m:home`)

Market key encoding: `s;{period};{type}` where period 0 = full game, type `m` = moneyline, `s` = spread, `ou` = total, `tt` = team total.

---

## Engine REST API (port 3401)

| Route | Method | Description |
|-------|--------|-------------|
| `/api/balances` | GET | All venue balances |
| `/api/balances/deposit` | POST | Add funds `{ venue, amount }` |
| `/api/balances/withdraw` | POST | Remove funds `{ venue, amount }` |
| `/api/balances/reset` | POST | Reset all to initial |
| `/api/trades` | GET | Last 200 trades |
| `/api/arbs` | GET | Last 200 detected arbs |
| `/api/stats` | GET | P&L, count, avg arb % |
| `/api/settings` | GET | Current settings |
| `/api/settings` | POST | Update settings `{ enabled, min_arb_pct, max_stake, stake_mode, fixed_stake, proportional_pct }` |
| `/api/reset` | POST | Full DB reset |
| `/ws` | WS | Real-time event stream |
