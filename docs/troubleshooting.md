# Troubleshooting

This is the current operations runbook.

## Startup Checklist

Use this order:

1. Start DK proxy.
2. Start bet placer.
3. Log in to sportsbook browsers.
4. Start Rust engine.

Commands:

```powershell
cd C:\dev\sportsbook-arbitrage
node .\dk-proxy.js
```

```powershell
cd C:\dev\sportsbook-arbitrage\bet-placer
npm start
```

```powershell
cd C:\dev\sportsbook-arbitrage\engine
$env:LIVE_MODE = "1"
$env:PIN_ODDS_SOURCE = "auth"
$env:FIXED_STAKE = "5"
$env:MAX_STAKE = "5"
cargo run --bin arb-engine
```

Confirm:

- DK proxy listens on `3403`;
- bet placer listens on `3402`;
- Pinnacle auth odds bridge listens on `3404`;
- bet placer opens Google Chrome sportsbook windows;
- engine prints `Arb Engine V6`;
- engine settings include `pin_odds_source=auth`;
- live mode prints WebSocket connection and live balances.

## Chrome Browser Does Not Launch

Current behavior:

- The bet placer uses Patchright with the `chrome` channel.
- Google Chrome must be installed and discoverable by Patchright.
- Each book uses a persistent Chrome profile under `data\profiles\<bookId>`.

Checks:

- Start only the bet placer first:

```powershell
cd C:\dev\sportsbook-arbitrage\bet-placer
npm start
```

- Confirm Chrome windows open for DraftKings, FanDuel, and Pinnacle.
- If Chrome launches but balances fail, log in again in each opened Chrome window and retry the `balance` command.
- If Chrome does not launch at all, inspect `bet-placer\src\browser-manager.js` and confirm the channel is `chrome`.
- Microsoft Edge is not required for the current runtime unless the browser channel is changed to `msedge`.

## Live Mode Acts Like Paper Mode

Cause:

- The Rust engine only gets a live WebSocket during startup.
- If it cannot connect to `WS_URL`, it falls back to paper execution.

Checks:

- Bet placer terminal shows `[WS] Engine connected`.
- Engine terminal shows `WebSocket connected to bet placer`.
- `LIVE_MODE=1` was set in the same PowerShell process that ran Cargo.

Fix:

- Start/restart bet placer.
- Restart the Rust engine.

## No Pinnacle Candidates

Default Pinnacle scanning is authenticated.

Checks:

- Bet placer is running.
- Bet placer printed `[PIN-ODDS] Listening on http://localhost:3404`.
- Pinnacle browser is logged in.
- `PIN_ODDS_SOURCE` is unset or set to `auth`.
- `PIN_AUTH_ODDS_BASE_URL` points to the active bridge.

Guest scan is explicit:

```powershell
$env:PIN_ODDS_SOURCE = "guest"
```

Use guest mode only for diagnostics.

## Expanded Markets Or Props Missing

Current behavior:

- `EXPANDED_MARKET_SCAN` is enabled by default.
- DK props require the DK proxy route `/dk-event/{eventId}/{subcategoryId}`.
- FD props require event-page payloads from discovered events.
- PIN props require related matchups/markets from the authenticated snapshot bridge or guest related endpoints.

Checks:

- DK proxy is running on `3403`.
- Bet placer is running on `3402` and Pinnacle auth odds bridge is running on `3404`.
- Pinnacle browser is logged in.
- `PIN_SNAPSHOT_RELATED` is unset or truthy.
- `PIN_RELATED_MAX_MATCHUPS` and `EXPANDED_MARKET_MAX_EVENTS_PER_SPORT` are not set too low.
- `DETECTOR_TELEMETRY=1` is enabled and `engine\data\events.jsonl` has `DETECTOR_MARKET_COUNTS`.

Diagnostic run:

```powershell
cd C:\dev\sportsbook-arbitrage\engine
$env:DETECTOR_TELEMETRY = "1"
$env:EXPANDED_MARKET_SCAN = "1"
$env:EXPANDED_MARKET_MAX_EVENTS_PER_SPORT = "8"
cargo run --bin arb-engine
```

## Prop Candidate Prints But Does Not Place

This is expected with the default live gate.

Current behavior:

- `LIVE_ALLOWED_MARKET_TYPES` defaults to `moneyline,spread,total,team_total`.
- `player_prop` can scan, detect, and paper trade, but is skipped before live WebSocket execution by default.
- Pinnacle special/player-prop IDs use the `SP:` prefix and are not yet wired to a live special-bet quote/place path.

Do not add `player_prop` to `LIVE_ALLOWED_MARKET_TYPES` until Pinnacle special quote/place, soft-book prop validation, and prop-aware recovery are implemented.

## FanDuel Markets Fetch But No FD/PIN Arbs

Current behavior:

- FD non-soccer markets use their sport page.
- FD soccer is fetched once as event type `1`, then supported competitions are mapped to league namespaces such as `MLS`, `EPL`, and `UCL`.
- FD scan prices are public. Authenticated FD price is checked later through `validate_soft`.

First diagnostic run:

```powershell
cd C:\dev\sportsbook-arbitrage\engine
$env:DETECTOR_TELEMETRY = "1"
cargo run --bin arb-engine
```

Inspect:

```text
C:\dev\sportsbook-arbitrage\engine\data\events.jsonl
```

Relevant event names:

- `DETECTOR_MARKET_COUNTS`;
- `DETECTOR_FUNNEL_SUMMARY`;
- `DETECTOR_PIN_PAIR_SNAPSHOT`;
- `DETECTOR_CANDIDATE`;
- `DETECTOR_CANDIDATE_SUMMARY`;
- `MIN_EDGE_SKIPPED`;
- `FAMILY_DEDUP_PASSED`;
- `FAMILY_DEDUP_SKIP`;
- `FAMILY_DEDUP_LOCKED`;
- `FAMILY_DEDUP_SUCCESS_RECORDED`.

Interpretation:

- No FD market count means the source fetch/parser is the issue.
- FD markets but no FD/PIN shared identity keys means event identity or league mapping is the issue.
- Shared identity keys but no complementary pairs means selection, line, or period normalization is the issue.
- Candidate rows but no fire usually means min edge, family dedup, quote, validation, or pre-fire checks killed the trade.
- `FAMILY_DEDUP_SKIP` with `policy=success_cooldown` means the family is waiting out the post-fill cooldown.
- `FAMILY_DEDUP_SKIP` with `policy=success_repeat_limit` means the family reached the configured successful repeat count.
- `FAMILY_DEDUP_SKIP` with `policy=hard_lock` means a clear soft-book limit/exposure or unavailable-market failure locked the family.

Do not loosen matching or safety gates before checking these telemetry events.

## Repeated High-Percent Pinnacle Arbs

Likely causes:

- stale source odds;
- period mismatch;
- unsupported Pinnacle period entering the book;
- three-way market mismatch;
- team normalization mismatch;
- spread sign issue.

Current safety behavior:

- Unknown Pinnacle period numbers are skipped instead of mapped to `full_game`.
- Non-full-game Pinnacle moneylines are skipped at parse time.

Immediate checks:

- Look at `venue_market_id`.
- Check the Pinnacle period segment in IDs like `s;10;m`; only `s;0;m` is a full-game Pinnacle moneyline.
- Compare the authenticated Pinnacle quote price against the scanned price.
- Use `PIN_ODDS_SOURCE=auth` for live testing.

## DraftKings Odds Fetch Is Slow Or Failing

Checks:

```powershell
cd C:\dev\sportsbook-arbitrage
node .\dk-proxy.js
```

Open:

```text
http://localhost:3403/dk/42648
```

Current deadlines:

- Rust odds request: `ODDS_FETCH_TIMEOUT_MS`, default `2500`;
- DK proxy upstream request: `DK_PROXY_FETCH_TIMEOUT_MS`, default `2500`.

If a DK league repeatedly fails, it should log a fetch warning and the poll cycle should continue.

## Pinnacle Bet Appears Accepted But Rust Times Out

This should not happen with the current fill ID normalization.

Expected successful chain:

1. Engine logs `Sending PLACE_BET via WS venue=pinnacle`.
2. Bet placer logs `[PIN] Bet accepted! id=...`.
3. Bet placer logs `[BET] <attempt_id> FILLED | pinnacle`.
4. Rust receives `fill_report`.
5. Rust logs `Fill received via WS`.

Checks if it regresses:

- Open the bet placer terminal output and find the matching `attempt_id`.
- Confirm the `fill_report` has the same `attempt_id`.
- Confirm `bet_id` and `purchase_id` are strings or numeric values, not objects.
- Inspect `engine\data\bets\<attempt_id>.jsonl` for `LEG2_ERROR` or `RESCUE_ERROR`.
- Run `node --check bet-placer\index.js`.
- Run `cargo test` from `engine\`.

The engine must not resolve this by creating a manual unknown-fill state. Automated filled/rejected/error truth is required.

## Engine Placed Extra Pinnacle Bets

Cause pattern:

- Rust times out waiting for the first Pinnacle fill.
- Recovery sends one rescue Pinnacle placement.
- The naked-leg monitor may later retry original-line Pinnacle rescue if the exposure was persisted.

Current prevention:

- Pinnacle accepted IDs are normalized and parseable.
- Rust accepts numeric fill IDs.

If extra placements occur again, treat it as a fill-report protocol bug until proven otherwise.

## Cashout Did Not Print In Terminal

Current behavior:

- Cashout commands/results are written to telemetry.
- They are not always printed as terminal info lines.

Where to check:

```text
C:\dev\sportsbook-arbitrage\engine\data\events.jsonl
C:\dev\sportsbook-arbitrage\engine\data\bets\<attempt_id>.jsonl
```

Relevant event names:

- `CASHOUT_RESULT`
- `CASHOUT_ERROR`
- `NAKED_LEG_CASHOUT_RESULT`
- `NAKED_LEG_CASHOUT_ERROR`

If the payload says `No cashout offer available`, the adapter did not submit a sportsbook cashout write.

## DK Cashout Not Submitted

Current DK behavior:

- DK `cashOut` requires a cashout offer.
- Without an offer, the adapter returns an error before sending the cashout request.

Check:

- `NAKED_LEG_STATUS_CHECKED` payload;
- `cashout_available`;
- `cashout_offer`;
- `CASHOUT_RESULT.error`.

## FD Cashout Fails

Current FD behavior:

- `getOpenBets()` fetches fresh open bets and `cashOutToken`.
- `cashOut(betId)` sends `cashoutBet` from the browser page context.
- Geo token source is `geolocation_document.jwt`, with fallback to `location_token.jwt`.

Checks:

- FanDuel browser is logged in.
- FD page still has valid geo state.
- `getOpenBets` finds the bet ID.
- The bet has a `cashOutToken`.
- The cashout write does not hit `FD_WRITE_TIMEOUT_MS`.

## One-Sided Exposure

Current protection stack:

- authenticated Pinnacle quote before risk;
- DK/FD pre-fire validation;
- Pinnacle pre-fire recheck;
- actual-fill hedge math;
- rescue/cashout/naked-leg recovery.

Immediate operator checks:

- Inspect `engine\data\bets\<attempt_id>.jsonl`;
- check account pages manually while running low stakes;
- clear false naked legs from `data\naked-legs.json` only after confirming all bets are settled or manually resolved.

## Naked-Leg Monitor

Current behavior:

- Monitor runs every `NAKED_LEG_MONITOR_CYCLES`, default `10`;
- reloads persisted legs;
- checks status;
- retries original-line Pinnacle rescue;
- attempts cashout when available;
- removes resolved entries.

The file is:

```text
C:\dev\sportsbook-arbitrage\engine\data\naked-legs.json
```

## FanDuel Balance Looks Wrong

Current adapter behavior:

- extracts username from session data when possible;
- scrapes visible page text;
- picks the largest dollar-looking amount.

Check the FanDuel account manually before live execution.

## REST API Exposure

Current behavior:

- Rust binds to `0.0.0.0:3401`;
- CORS is permissive;
- there is no auth.

Mitigation:

- keep it on a trusted local machine/network;
- use Windows firewall rules if needed;
- do not port-forward the service.

## Keepalive Reload Interference

The bet placer reloads running sportsbook pages every 5 minutes:

```text
KEEPALIVE_INTERVAL_MS = 5 * 60 * 1000
```

If a browser is being manually inspected during live testing, the reload can interrupt what you are looking at.

## Validation Commands

```powershell
cd C:\dev\sportsbook-arbitrage\engine
cargo test
```

```powershell
cd C:\dev\sportsbook-arbitrage
node --check .\dk-proxy.js
node --check .\bet-placer\index.js
node --check .\bet-placer\src\browser-manager.js
node --check .\bet-placer\src\sportsbooks\draftkings.js
node --check .\bet-placer\src\sportsbooks\fanduel.js
node --check .\bet-placer\src\sportsbooks\pinnacle.js
git diff --check
```
