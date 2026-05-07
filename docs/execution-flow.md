# Execution Flow (Current State)

This document traces what happens from public odds polling through paper or live execution. It reflects the current Rust and Node implementation, not the older `config.toml` policy draft.

## High-Level Loop

The main loop runs inside `engine/src/main.rs`.

```text
startup
  open SQLite at data/trades.db
  seed balances/settings if missing
  apply env setting overrides
  start dashboard broadcaster
  build reqwest client
  if LIVE_MODE=1, try WebSocket connection to bet placer
  if PIN_ODDS_SOURCE is auth/default, Pinnacle scan expects bet placer HTTP on 3404
  start polling task
  start Axum REST/WebSocket server on 0.0.0.0:3401

poll cycle, every 5 seconds
  fetch all configured venue odds concurrently
  update in-memory Book
  detect arb candidates
  persist arb opportunities
  broadcast dashboard events
  run execution gates
  execute eligible candidates in paper or live mode
  periodically refresh live balances
```

If `LIVE_MODE=1` but the engine cannot connect to `WS_URL` during startup, it logs a warning and continues without a live WebSocket connection. In that state, execution uses the paper executor.

## Odds Collection

`odds_fetcher::fetch_all` fetches odds for the sports configured in `engine/src/config.rs`.

Current source behavior:

- DraftKings: Rust fetches through `http://localhost:3403/dk/{leagueId}` first. The proxy calls DraftKings with Node `fetch`. Rust falls back to the direct DraftKings URL if needed.
- FanDuel: Rust calls Ontario FanDuel content-managed-page endpoints for public odds.
- Pinnacle: Rust defaults to authenticated odds. It calls `GET /pinnacle/snapshot/{leagueId}` on `PIN_AUTH_ODDS_BASE_URL` or `http://127.0.0.1:3404`; the bet placer then uses the single logged-in Pinnacle browser session to fetch Arcadia `matchups` and `markets/straight`.
- Pinnacle guest scanning is still present but opt-in with `PIN_ODDS_SOURCE=guest` or `PIN_ODDS_SOURCE=public`.
- All venue fetches use `ODDS_FETCH_TIMEOUT_MS`, default `2500`, so one slow or dead league should deadline out quickly instead of delaying arb detection for the old 15-second request timeout.

All responses are converted into `CanonicalMarket` records with normalized event IDs, selections, period labels, line values, venue IDs, and timestamps.

## Normalization

Team and selection matching is handled by `engine/src/team_names.rs`.

Canonical event IDs:

```text
{SPORT}:{YYYY-MM-DDTHH}:{TEAM_A}-vs-{TEAM_B}
```

`TEAM_A` and `TEAM_B` are sorted alphabetically after normalization. The suffix is therefore a stable pair key, not guaranteed away-vs-home order.

Examples:

```text
NBA:2026-03-15T19:BOS-vs-GSW
NHL:2026-03-15T20:BOS-vs-NYR
```

If the source start time cannot be parsed into a date/hour, the code falls back to an alphanumeric hash derived from the source start time.

Period normalization:

- DK/FD labels containing first/second half, quarters, periods, `60 min`, `3 way`, or `3-way` map to explicit period strings.
- Pinnacle period numbers map in code, with period `6` currently treated as `regulation`.
- Unmatched labels default to `full_game`.

Spread identity uses absolute line values for matching, then detector logic verifies opposite signs before accepting the candidate.

## Arb Detection

`arb_detector::detect_arbs` groups in-memory prices by market identity and searches two complementary selections.

Detection rules:

- Candidate data must be fresh according to `MAX_DATA_AGE_MS` from `config.rs` (currently 30000 ms).
- Candidate must have at least two selections.
- Soccer-style three-way moneyline sports are skipped for two-leg moneyline arbs.
- Partial-period moneylines are skipped.
- Moneyline complements must be different non-draw selections.
- Totals and team totals require `over` vs `under`.
- Spreads and alternate spreads require different selections and opposite line signs.
- Current runtime requires at least one Pinnacle leg.

Math:

```text
sigma = 1 / leg1_odds + 1 / leg2_odds
arb_pct = 1 - sigma
roi = 1 / sigma - 1
```

Only candidates with `sigma < 1.0` are arbs. Candidates are sorted by `arb_pct` descending.

## Pre-Execution Gates

The main loop applies these gates before calling the executor:

1. Persist and broadcast the detected opportunity.
2. Check the in-memory circuit breaker.
3. Check the per-venue rate limiter.
4. Verify both candidate legs are still present in the current book.
5. Reject if either leg has drifted more than 5% from detection-time odds.
6. Warn but do not automatically reject very high `arb_pct` values.
7. Skip if `SKIP_LIVE=1` and the candidate is live.
8. Skip pregame events more than `MAX_HOURS_TO_START` hours away. The default is 24.
9. Skip if the candidate's dedup key has already traded.
10. Require `settings.enabled`.
11. Call the paper or live executor, which enforces `settings.min_arb_pct` and stake sizing constraints.

The current first-wave dedup key is:

```text
event_id | market_type | leg1_venue | leg2_venue
```

This is intentionally still basic. Hunter-style family/ladder dedup is shelved for the next merge phase, so this key can still refire the same family after the 60-second cooldown if line or price churn produces repeated candidates.

## Stake Sizing

`stake_optimizer.rs` sizes the two legs to equalize payout.

Given total capital:

```text
leg1_stake = capital * (1 / leg1_odds) / sigma
leg2_stake = capital * (1 / leg2_odds) / sigma
guaranteed_payout = capital / sigma
guaranteed_profit = guaranteed_payout - capital
```

Stakes and results are rounded to cents.

`requested_capital_with_soft_cap` first chooses a target soft-book stake:

- fixed mode starts from `settings.fixed_stake`;
- proportional mode uses `settings.proportional_pct` of the soft-book balance, or the smaller balance for non-Pinnacle pairs;
- the target soft-book stake is capped by `settings.max_stake`.

For Pinnacle-backed arbs, `settings.max_stake` is a soft-book cap, not a Pinnacle hedge cap. The requested capital is derived so the non-Pinnacle leg lands near the requested soft stake.

`compute_stakes_with_balances` then scales capital down when a leg would exceed:

- the venue balance stored in SQLite, or
- `settings.max_stake` on a non-Pinnacle soft-book leg.

Pinnacle hedge legs are not capped by `MAX_STAKE` at this stage. They are bounded later by authenticated quote max stake, Pinnacle balance, and profitability checks.

It skips plans below:

- `MIN_CAPITAL = 1.0`
- `MIN_PROFIT = 0.01`

Settings defaults are seeded in SQLite:

| Setting | Default |
| --- | ---: |
| `enabled` | `true` |
| `min_arb_pct` | `0.5` |
| `max_stake` | `50` |
| `stake_mode` | `fixed` |
| `fixed_stake` | `25` |
| `proportional_pct` | `5` |

For non-Pinnacle pairs, fixed mode still requests `fixed_stake * 2` total capital and proportional mode is capped by `max_stake * 2`. The production live path currently requires a Pinnacle leg, so the soft-cap behavior above is the important one for live testing.

## Paper Execution

`paper_executor::try_execute`:

1. Checks `settings.min_arb_pct`.
2. Reads both venue balances from SQLite.
3. Computes a balance-aware stake plan.
4. Deducts both stakes.
5. Immediately credits back stake plus theoretical profit share.
6. Inserts a `paper_trades` row with status `executed`.

Paper execution is a simulator. It does not model sportsbook rejection, settlement delay, voids, pushes, odds movement during placement, or one-sided exposure.

## Live Execution

`live_executor::try_execute` is used only when the engine has an active WebSocket connection to the Node bet placer.

Supported live venues:

```text
draftkings
fanduel
pinnacle
```

Live execution steps:

1. Check `settings.min_arb_pct`.
2. Read current venue balances from SQLite.
3. Compute a balance-aware stake plan using `MAX_STAKE` as the soft-book cap.
4. Choose leg order:
   - If one leg is Pinnacle, place the non-Pinnacle leg first.
   - For FanDuel vs DraftKings, place FanDuel first.
5. If Pinnacle is involved, send `quote_bet` before either leg is placed.
6. Validate quoted Pinnacle price with quote sigma.
7. Adjust stake to Pinnacle min/max limits if needed and recalculate the other leg.
8. If leg 1 is DraftKings or FanDuel, send `validate_soft` and use the fresh soft price for sigma and hedge math.
9. If Pinnacle is leg 2, send the short `PIN_PREFIRE_RECHECK` quote immediately before leg 1 risk.
10. Place leg 1 with `place_bet`.
11. Drain best-effort `balance_update` messages.
12. Recompute Pinnacle hedge from the actual filled soft stake and odds.
13. Abort leg 2 and enter recovery if post-fill sigma/profitability is no longer acceptable.
14. Place leg 2 with `place_bet`.
15. Drain best-effort balance updates.
16. Insert a trade with status `executed_live`.

If leg 1 rejects, leg 2 is not placed. If leg 2 rejects after leg 1 fills, or if post-fill checks kill the hedge, the engine enters recovery. Recovery can try Pinnacle rescue quotes, compare cashout economics, attempt DK/FD cashout, and persist unresolved exposure to `data/naked-legs.json`.

## Engine-to-Bet-Placer WebSocket

Default endpoint:

```text
ws://127.0.0.1:3402
```

Request types from Rust:

- `get_balances`
- `validate_soft`
- `quote_bet`
- `place_bet`
- `get_bet_status`
- `cashout`
- `ping`

Response types from Node:

- `balances`
- `validate_soft_result`
- `quote_result`
- `fill_report`
- `bet_status`
- `cashout_result`
- `balance_update`
- `pong`

The Rust reader waits for messages matching the expected type and `attempt_id` where applicable. There is no reconnect loop after the initial connection.

## Balance Flow

Startup in live mode:

1. Rust connects to the bet placer.
2. Rust sends `get_balances`.
3. Node asks each sportsbook adapter for balance.
4. Rust writes balances into SQLite.
5. Rust broadcasts `balance_updated` to dashboard clients.

After live fills:

- The Node adapter sends a best-effort `balance_update` for that venue.
- Rust drains pending `balance_update` messages after each leg and updates SQLite.

Periodic refresh:

- `BALANCE_REFRESH_CYCLES` controls how often the polling loop asks the bet placer for fresh balances.
- Default is every 60 poll cycles, which is roughly every 5 minutes at the current 5 second poll interval.

## Recovery Flow

Recovery is entered only after the first live leg has filled and the normal second-leg path cannot safely continue.

Current behavior:

1. Fetch soft-book bet status when `purchase_id` or `bet_id` is available.
2. Capture any DK/FD cashout offer.
3. If a Pinnacle hedge ID exists, quote the original PIN market and nearby total/spread/team-total lines.
4. Stop retrying after `RESCUE_PIN_HARD_YANK_THRESHOLD` hard-yank errors, default 3.
5. Skip a rescue quote if `MAX_RESCUE_LOSS` is set and the rescue loss is above the limit.
6. Prefer cashout over rescue when the cashout loss is smaller.
7. If rescue does not fill, send a cashout command where supported.
8. If unresolved, write a naked-leg ledger entry.

The polling loop monitors persisted naked legs every `NAKED_LEG_MONITOR_CYCLES` cycles, default 10. The monitor rechecks status/cashout, tries original-line Pinnacle relist recovery, and removes resolved entries.

## Dashboard Broadcasts

The engine broadcasts these WebSocket events to dashboard clients connected to `/ws`:

- `arb_detected`
- `trade_executed`
- `balance_updated`
- `settings_updated`
- `poll_status`
- `error`

These dashboard events are separate from the engine-to-bet-placer WebSocket protocol.

## Current Operational Caveats

- `engine/config.toml` is not the active policy source for the main loop.
- REST and dashboard WebSocket bind to `0.0.0.0:3401` and have no auth.
- Default Pinnacle scanning requires the bet placer HTTP bridge and logged-in Pinnacle session unless `PIN_ODDS_SOURCE=guest` is set.
- `cargo test` currently passes, but warnings remain from unused helper/legacy modules.
- The richer state machine, treasury model, policy helper, and older `rescue_hedger.rs` calculator are present but not the main execution path.
- A live WebSocket startup failure causes paper fallback.
- There is no automatic live WebSocket reconnect.
- Bet placer keepalive reloads sportsbook pages every 5 minutes, which can interfere with browser state during an active operation.
