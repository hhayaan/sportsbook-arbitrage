# Execution Flow

This document describes the current execution path from odds polling to paper or live trading.

## Startup

The Rust entry point is `engine/src/main.rs`.

```text
start
  initialize tracing
  print Arb Engine V6 mode line
  open SQLite at data/trades.db
  load SQLite settings
  apply startup env overrides
  print settings, DK proxy endpoint, and Pinnacle odds source
  build REST/dashboard state
  build reqwest odds client
  spawn polling task
  bind REST/dashboard server on 0.0.0.0:3401
```

In live mode, the polling task connects once to the bet placer WebSocket:

```text
WS_URL or ws://127.0.0.1:3402
```

If that startup connection fails, the engine logs the failure and executes through paper mode because no live WebSocket connection exists.

## Poll Cycle

The loop runs every `config::POLL_INTERVAL_SECS`, currently 5 seconds.

```text
poll
  fetch all configured odds concurrently
  update changed markets in the in-memory Book
  touch unchanged markets so live snapshots do not expire
  expire stale book entries
  prune rate limiter state
  detect arbs
  persist detected opportunities
  broadcast dashboard events
  apply pre-execution gates and family dedup
  run paper or live executor
  periodically refresh live balances
  periodically monitor persisted naked legs
  sleep 5 seconds
```

Slow individual odds fetches use `ODDS_FETCH_TIMEOUT_MS`, default `2500`. A dead DK league should deadline quickly instead of holding detection for the full HTTP client timeout.

## Odds Fetching

DraftKings:

- Rust asks the local DK proxy first: `http://127.0.0.1:{DK_PROXY_PORT}/dk/{leagueId}`.
- The proxy uses Node `fetch`.
- Rust falls back to direct DK public odds fetch if needed.

FanDuel:

- Rust fetches public Ontario content endpoints.
- FanDuel soccer is fetched as one event type, then supported competitions are mapped to the same league namespaces used by DK/PIN: `MLS`, `EPL`, `LA_LIGA`, `SERIE_A`, `BUNDESLIGA`, `LIGUE_1`, and `UCL`.
- Unsupported FD soccer competitions are skipped.
- FanDuel authenticated session checks happen only during live validation/cashout.

Pinnacle:

- Default source is authenticated: `PIN_ODDS_SOURCE=auth`.
- Rust calls the bet-placer HTTP bridge at `PIN_AUTH_ODDS_BASE_URL`, default `http://127.0.0.1:3404`.
- The bridge uses the logged-in Pinnacle browser session and authenticated Arcadia endpoints.
- Guest scan is opt-in with `PIN_ODDS_SOURCE=guest` or `PIN_ODDS_SOURCE=public`.

## Normalization

Each source price becomes a `CanonicalMarket`. Team names and selections are normalized by `team_names.rs`.

Canonical event IDs look like:

```text
MLB:2026-05-07T19:COL-vs-NYM
```

The team pair is sorted. It is a cross-book identity key, not an away/home declaration.

Period labels are normalized in the fetcher. Unsupported Pinnacle period numbers are skipped instead of silently becoming `full_game`, and non-full-game Pinnacle moneylines are dropped before detection.

## Detection

`arb_detector::detect_arbs` groups the book by market identity and searches two complementary selections.

Current detection constraints:

- at least one leg must be Pinnacle;
- moneyline draw selections are rejected;
- soccer-style three-way moneylines are skipped;
- partial-period moneylines are skipped;
- totals require over vs under;
- spreads require different selections and opposite line signs;
- candidates must be fresh according to `MAX_DATA_AGE_MS`, currently `30000`.

Math:

```text
sigma = 1 / leg1_odds + 1 / leg2_odds
arb_pct = 1 - sigma
```

Only `sigma < 1.0` can trade.

Optional detector diagnostics are written to telemetry when `DETECTOR_TELEMETRY=1` or `DETECTOR_DIAGNOSTICS=1`:

- `DETECTOR_MARKET_COUNTS` counts source markets by venue, sport, market type, and period.
- `DETECTOR_FUNNEL_SUMMARY` counts shared identity keys, complementary pairs, rejects, near misses, and candidates by venue pair.
- `DETECTOR_PIN_PAIR_SNAPSHOT` records compact Pinnacle-involved candidate and near-miss rows, including venue event IDs and venue market IDs.
- `DETECTOR_CANDIDATE` and `DETECTOR_CANDIDATE_SUMMARY` show candidates before and after the min-edge threshold.
- `MIN_EDGE_SKIPPED` records candidates that were detected but skipped by configured edge.
- `FAMILY_DEDUP_PASSED` and `FAMILY_DEDUP_SKIP` show whether the live-fire family gate allowed or blocked the candidate.
- `FAMILY_DEDUP_LOCKED` records hard locks from clear soft-book limit/exposure or unavailable-market failures.
- `FAMILY_DEDUP_SUCCESS_RECORDED` records successful-fill repeat state.

## Pre-Execution Gates

Before an executor is called, the main loop:

1. Inserts the opportunity into SQLite.
2. Broadcasts `arb_detected`.
3. Checks the circuit breaker.
4. Checks the per-venue rate limiter.
5. Verifies both legs still exist in the current in-memory book.
6. Rejects if either leg drifted more than 5%.
7. Emits a warning for very high arb percentages.
8. Skips live markets when `SKIP_LIVE=1`.
9. Skips pregame events farther than `MAX_HOURS_TO_START`, default 24 hours.
10. Requires `settings.enabled`.
11. In live mode, applies `LIVE_ALLOWED_PAIRS` if set; this is placement-only and does not affect scanning or `[ARB]` terminal output.
12. Applies persistent family-scope dedup for actual live WebSocket execution.
13. Calls paper or live execution.

The family dedup key is:

```text
sport + event_id + soft_venue + market_type + period
```

The key is order-independent for Pinnacle/soft-book pairs and intentionally excludes line and side. A successful live fill records the family, starts a 30-second success cooldown by default, and allows up to 3 successful repeats after the first fill. Clear soft leg-1 limit/exposure or unavailable-market failures hard-lock the family for the 6-hour TTL. Generic `PurchaseNotAccepted`, slippage, stale IDs, auth/browser/network errors, insufficient funds, Pinnacle quote failures, and Pinnacle placement failures do not hard-lock the family. State is stored in `data/family-dedup.json` by default. Paper/scan runs do not write persistent family state.

## Stake Sizing

The staking target is an equal-payout two-leg plan:

```text
leg1_stake = capital * (1 / leg1_odds) / sigma
leg2_stake = capital * (1 / leg2_odds) / sigma
```

For Pinnacle-backed live arbs:

- fixed mode starts from `FIXED_STAKE` or SQLite `fixed_stake`;
- `MAX_STAKE` caps the non-Pinnacle soft leg;
- requested capital is derived so the soft leg lands near the desired stake;
- Pinnacle hedge is not capped by `MAX_STAKE`;
- Pinnacle hedge is later bounded by authenticated quote max, Pinnacle balance, and profitability.

Plans below minimum capital/profit are skipped.

## Paper Execution

`paper_executor::try_execute`:

1. Checks minimum arb percent.
2. Reads SQLite balances.
3. Computes a balance-aware plan.
4. Deducts both stakes.
5. Credits simulated payout/profit immediately.
6. Inserts a trade row with status `executed`.

Paper execution is only a simulator.

## Live Execution

`live_executor::try_execute` runs only when a bet-placer WebSocket connection exists.

Current live steps for Pinnacle-backed arbs:

1. Check minimum arb percent.
2. Read SQLite balances.
3. Compute a stake plan with the soft-book cap.
4. Send Pinnacle `quote_bet`.
5. Validate quote price/min/max.
6. Adjust stake plan to quote constraints when needed.
7. Send DK/FD `validate_soft` for the soft leg.
8. Recompute sigma using fresh soft price.
9. Send final short Pinnacle pre-fire quote before soft risk.
10. Send soft leg `place_bet`.
11. Drain balance updates.
12. Recompute Pinnacle hedge from actual filled soft stake/odds.
13. Send Pinnacle `place_bet`.
14. Drain balance updates.
15. Insert status `executed_live` only after both legs fill.

Fill outcomes are automated:

- `filled` completes the step;
- `rejected` returns an error path;
- timeout/error enters recovery when leg 1 already filled.

The engine does not use an operator-manual unknown fill state.

## Engine-to-Bet-Placer Protocol

Rust sends:

- `get_balances`
- `validate_soft`
- `quote_bet`
- `place_bet`
- `get_bet_status`
- `cashout`
- `ping`

Node responds:

- `balances`
- `validate_soft_result`
- `quote_result`
- `fill_report`
- `bet_status`
- `cashout_result`
- `balance_update`
- `pong`

`fill_report.purchase_id` and `fill_report.bet_id` are string-normalized by Node. Rust also accepts numeric IDs from adapter payloads.

## Recovery Flow

Recovery starts only when leg 1 filled and leg 2 cannot safely finish.

```text
recovery
  emit RESCUE_ENTER
  check soft-book status if purchase_id/bet_id exists
  store cashout offer when available
  quote original/adjacent PIN rescue markets
  stop after hard-yank threshold
  compare rescue loss with cashout loss
  place PIN rescue if viable
  if not rescued, send cashout where supported
  if not resolved, persist naked leg
```

Cashout and status results are written to telemetry:

```text
engine/data/events.jsonl
engine/data/bets/<attempt_id>.jsonl
```

They are not always printed as terminal lines.

## Naked-Leg Monitor

Every `NAKED_LEG_MONITOR_CYCLES`, default `10`, the polling loop calls `monitor_naked_legs` when live WebSocket is available.

The monitor:

- reloads `data/naked-legs.json`;
- checks soft-book status;
- removes confirmed settled/cashed-out exposures;
- quotes original Pinnacle market for relist recovery;
- attempts cashout when an offer is available;
- rewrites the ledger.

## Current Work To Do

- Decide whether true ladder buckets or an edge-improvement gate are needed after low-stake family-dedup testing.
- Review FD/PIN detector telemetry before changing non-soccer normalization or adding FD authenticated scan overlay.
- Keep adding replay fixtures for unsupported Pinnacle period handling so unknown periods cannot become full-game markets again.
- Add or improve terminal visibility for recovery cashout actions.
- Add incident replay fixtures for real live failures.
