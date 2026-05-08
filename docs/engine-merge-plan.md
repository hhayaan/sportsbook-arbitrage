# Engine Merge Plan

This file is the current merge status for bringing selected Hunter engine behavior into this Rust-first project. It is not a changelog. It describes what the engine includes now, what is next, and what is intentionally out.

## Guardrails

- Engine logic stays in Rust.
- Browser/session/API automation stays in `bet-placer` as an adapter bridge.
- Operator surface stays terminal-first with JSONL telemetry for incident review.
- Live market scope is moneyline, totals, and spreads.
- Player props, broad alternate markets, and frontend/UI surfaces are out.
- Live latency matters. Safety checks must have short timeouts and must reuse the existing logged-in sportsbook sessions.
- There is only one Pinnacle browser context, logged in by the user.

## Current Difference Inventory

| Area | Current Rust project status | Hunter capability or difference | Decision |
| --- | --- | --- | --- |
| Runtime shape | Rust engine plus Node adapters. | Hunter is a larger JavaScript runtime. | Keep Rust engine structure; do not port the monolith. |
| Operator surface | Terminal logs, REST/dashboard API, JSONL telemetry. | Hunter has broader operational surfaces. | Keep terminal-first operation. |
| Market scope | Moneyline, totals, spreads. | Hunter includes props, alts, and wider period experiments. | Keep current scope. |
| Pinnacle scan source | Authenticated Pinnacle odds are default through `PIN_ODDS_SOURCE=auth`. Guest scan is opt-in. | Hunter treats logged-in Pinnacle as executable truth. | Merged and keep. |
| Pinnacle executable quote | `quote_bet` is binding for hedge price, min stake, max stake. | Hunter does not trust discovery odds for execution. | Merged and keep. |
| Soft-book validation | DK/FD `validate_soft` runs before soft-leg risk. | Hunter refreshes soft odds from authenticated sessions before firing. | Merged and keep. |
| Final Pinnacle recheck | Short pre-fire quote before sending the soft leg. | Hunter rechecks PIN immediately before soft risk. | Merged and keep. |
| Stake sizing | `MAX_STAKE` caps only the soft-book stake in Pinnacle-backed arbs; Pinnacle hedge is quote, balance, and profit bounded. | Hunter sizes hedge from actual filled soft stake. | Merged and keep. |
| Pinnacle fill parsing | Node normalizes `purchase_id` and `bet_id` to strings; Rust also accepts numeric IDs. | Hunter's PIN IDs are numeric in accepted responses. | Merged after live timeout evidence. |
| DK status/cashout classifier | Rust has raw-status classification and DK bridge support. | Hunter fixed the `Unsettled` trap with raw open-first logic. | Merged and keep. |
| FD cashout | FD open-bet lookup, fresh `cashOutToken`, browser-context cashout write, and geo JWT freshness are implemented. | Hunter has a larger FD cashout stack. | Scoped core merged; extra FD stack is out unless evidence requires it. |
| Recovery | Original/adjacent Pinnacle rescue, cashout comparison, naked-leg persistence, monitor reload, and original-line relist recovery are wired. | Hunter has broader retry scheduling and settlement support. | First-wave core merged; retry schedule and settlement are not merged. |
| Attempt telemetry | JSONL global and per-attempt events are written under `engine/data` by default. | Hunter has structured lifecycle telemetry. | Rust-native telemetry merged. |
| Dedup/family/ladder | Current runtime has a basic 60-second event/market/venue-pair cooldown. | Hunter has family-scope dedup and ladder allowlists. | Only planned second-phase item. |
| Pinnacle period handling | Source currently maps unknown Pinnacle periods to `full_game`. | Hunter preserves period suffixes in family/ladder keys. | Needs fix before higher-stake live use. |
| Settlement ingest | SQLite records live trades, balances, arbs, and telemetry; no settlement reconciliation pipeline. | Hunter has more settlement/open-bet handling. | Not merged. |
| Replay fixtures | Focused Rust unit tests exist; no incident replay harness. | Hunter migration docs call for replay fixtures. | Recommended later, not phase two. |
| Unknown fill/manual reconciliation | Fill status is not allowed to become an operator-manual "unknown placed" state. | Hunter had an indeterminate-write path that could send `fill_report status="unknown"` with `retry_allowed=false`. | Not being merged. Automated filled/rejected/error truth is required. |

## Merged First Wave

### Authenticated Pinnacle Odds And Quote Truth

Current behavior:

- Rust defaults to `PIN_ODDS_SOURCE=auth`.
- Rust calls `GET /pinnacle/snapshot/{leagueId}` on the bet-placer HTTP bridge, default `http://127.0.0.1:3404`.
- The bet placer uses the single logged-in Pinnacle browser session to fetch Arcadia `matchups` and `markets/straight`.
- Guest Pinnacle scanning remains available only with `PIN_ODDS_SOURCE=guest` or `PIN_ODDS_SOURCE=public`.
- Before execution, Rust uses authenticated Pinnacle `quote_bet` as the hedge truth for price, min stake, and max stake.

Left out:

- A second Pinnacle browser.
- Guest/auth reconciliation.
- Automatic fallback from auth scan to guest scan.
- Hunter's family-level quote cache and umbrella cooldowns.

Recommendation:

- Keep authenticated scan as default.
- Remove guest scan later if authenticated scan stays stable and a leaner codebase is preferred.

### DK / FD Soft Pre-Fire Validation

Current behavior:

- Rust sends `validate_soft` before risking a DK or FD soft leg.
- DraftKings validation uses authenticated `calculateBets`.
- FanDuel validation uses authenticated price refresh through `getSelectionInfo`.
- Rust uses the fresh soft price for sigma checks and downstream hedge math.

Left out:

- DK NewOffer filled-leg handoff.
- Large validation retry taxonomy.

Recommendation:

- Keep the current bounded validation.
- Add NewOffer handling only if live evidence shows DK accepts a bet at changed terms that the current fill path cannot represent.

### Pinnacle Pre-Fire Recheck

Current behavior:

- Rust sends a final short Pinnacle quote immediately before `LEG1_SENT`.
- Default timeout is `PIN_PREFIRE_RECHECK_TIMEOUT_MS=1500`.
- Hard-yank errors abort before soft risk and temporarily blacklist the exact Pinnacle market ID.
- Timeout or unclassified transient failure fails open to avoid making live arbs too slow.

Left out:

- Hunter's family-scope hard-yank key.
- Matchup umbrella cooldown.

Recommendation:

- Keep exact-market blacklist now.
- Revisit family-level yank behavior with the dedup/family phase.

### Actual-Fill Hedge Math

Current behavior:

- After the soft leg fills, Rust recomputes the Pinnacle hedge from the filled soft stake and filled soft odds.
- Hedge sizing uses:

```text
pin_stake = soft_filled_stake * soft_filled_decimal_odds / authenticated_pin_decimal_odds
```

- Rust checks quote max, Pinnacle balance, sigma, and guaranteed profit after rounding.

Left out:

- Nothing material for this scope.

Recommendation:

- Keep permanently.

### Pinnacle Fill ID Normalization

Current behavior:

- The bet placer converts `purchase_id` and `bet_id` to strings in `fill_report`.
- Rust also accepts numeric IDs for `fill_report` and `bet_status`.
- This prevents an accepted Pinnacle bet with numeric `bet_id` from being ignored by Rust's JSON parser.

Left out:

- Hunter's indeterminate Pinnacle write response: `fill_report status="unknown"` with `retry_allowed=false`.
- Manual "unknown fill" status.
- Any operator-required reconciliation stop.

Recommendation:

- Keep this minimal automated fix.
- Do not reintroduce unknown fill status as a placement outcome.

### Recovery Stack

Current behavior:

- If leg 1 fills and leg 2 cannot safely complete, Rust enters recovery.
- Pinnacle rescue can quote the original market and adjacent total/spread/team-total lines.
- Rescue stake balances payout against the filled soft leg.
- Rescue stops after `RESCUE_PIN_HARD_YANK_THRESHOLD`, default `3`, hard-yank errors.
- If a soft-book cashout offer exists and is cheaper than rescue loss, Rust prefers cashout.
- If rescue/cashout do not resolve the exposure, Rust writes `data/naked-legs.json`.
- The polling loop monitors persisted naked legs every `NAKED_LEG_MONITOR_CYCLES`, default `10`, and tries status, cashout, and original-line Pinnacle relist recovery.

Left out:

- Full Hunter cashout retry schedule.
- DK MyBets fallback.
- Settlement-ingest pipeline.
- Any manual "unknown placed" stop.

Recommendation:

- Keep first-wave recovery.
- Add terminal visibility for cashout commands/results if operator logs need to show every recovery action.

### DK Cashout / Status Classifier

Current behavior:

- `Unsettled`, `Not Settled`, `Open`, `Pending`, `Placed`, `Accepted`, `Processing`, and `LiveDelay` classify as open.
- `Cashed Out` is a confirmed cashout status.
- Won/lost/voided/refunded/canceled/push statuses classify as terminal non-cashout.
- DK `getBetStatus` uses `getPurchases`.
- DK `cashOut` requires a cashout offer and then confirms the result through raw status.

Left out:

- Settlement P&L ingest.
- MyBets fallback.

Recommendation:

- Keep classifier.
- Add settlement ingest later as accounting infrastructure.

### FD Cashout

Current behavior:

- `getOpenBets()` calls FanDuel `getEligibleBets?sortBy=START_TIME`.
- `getOpenBets()` extracts open bet IDs, status, offer, `cashOutToken`, and delay.
- `cashOut(betId)` fetches a fresh `cashOutToken` immediately before sending `cashoutBet`.
- FanDuel cashout writes run from the browser page context because the write needs fresh browser geo/session state.
- Geo headers read `localStorage.geolocation_document.jwt` first, with `location_token.jwt` only as fallback.
- FanDuel placement returns top-level bet ID and purchase ID values into the Rust fill path.

Left out:

- Hunter's TLS impersonation transport.
- FD UI fallback placement.
- FD DSC-0015 rolling hard-reset counter.
- FD player props, alt markets, partial-period surfaces, and large diagnostics stack.
- Standalone open-bets polling subsystem.

Recommendation:

- Keep scoped FD cashout.
- Add hard-reset handling only if logs show repeated geo-token or DSC-0015 failures.

### Structured Attempt Telemetry

Current behavior:

- Global telemetry writes to `data/events.jsonl`, or `ATTEMPT_TELEMETRY_DIR/events.jsonl`.
- Per-attempt telemetry writes to `data/bets/<attempt_id>.jsonl`.
- Events cover quote, validation, pre-fire recheck, leg sent/filled, recovery, cashout, naked leg, and monitor stages.

Left out:

- Hunter's exact event schema.
- Frontend telemetry views.

Recommendation:

- Keep Rust-native JSONL.
- Use per-attempt files as the first incident-review source.

## Shelved For Next Merge Phase

### Advanced Family / Ladder Dedup

This is the only planned phase-two merge item.

Required scope:

- Add a Rust family key for live fire dedup.
- Preserve period in the family key.
- Default to family-scope blocking.
- Add explicit ladder allowlist behavior only where configured.
- Log terminal-visible blocked/allowed decisions.
- Reuse the family identity for future family-level Pinnacle yank behavior.

Why it is next:

- It directly controls repeated exposure from the same event/market family.
- It does not require props, UI, new books, or a broader market scope.
- It is the cleanest next safety upgrade after first-wave live testing.

## Required Fixes Before Higher-Stake Live Use

These are not the phase-two Hunter dedup item, but they are current project work that should be addressed before raising stake size.

| Work item | Reason |
| --- | --- |
| Stop mapping unknown Pinnacle periods to `full_game` | Current parser falls back unknown period numbers to full game. That can make live period markets look like full-game markets. |
| Improve recovery terminal visibility | Cashout attempts/results are currently visible in JSONL telemetry, but not always obvious in terminal output. |
| Decide whether to remove unused unknown-fill fields | The unknown/manual reconciliation behavior is not being merged. Any unused remnants should be removed if they are not needed for real adapter payloads. |
| Add incident replay fixtures | Live issues should become deterministic tests before recovery logic grows. |

## Not Merged And Not Planned

| Feature | Decision |
| --- | --- |
| GUI/frontend port | Leave out. |
| Player props | Leave out until ML/totals/spreads are stable. |
| Full Hunter monolith | Leave out. |
| Hunter indeterminate Pinnacle write response | Leave out. Do not send `fill_report status="unknown"` / `retry_allowed=false` as a live placement outcome. The engine must receive automated filled/rejected/error truth. |
| Manual unknown-fill reconciliation status | Leave out. The engine must not require operator reconciliation as part of normal automation. |
| Second Pinnacle browser | Leave out. |
| Guest/auth Pinnacle double-scan | Leave out unless explicitly reintroduced for diagnostics. |
| FD TLS impersonation | Leave out unless browser-context writes fail with evidence. |
| FD UI fallback placement | Leave out unless API placement becomes unreliable with evidence. |
| FD DSC-0015 hard-reset counter | Leave out unless geo/PX failure logs justify it. |
| DK MyBets fallback | Not planned for phase two. Revisit with settlement work. |
| Full cashout retry schedule | Not planned for phase two. Revisit after live evidence. |
| Settlement ingest parity | Not planned for phase two. Add later for accounting correctness. |
| Full Hunter event schema | Leave out unless an external tool needs it. |
| Broad period expansion | Leave out until period identity is proven. |
| FD basketball alts and partial periods | Leave out. |
| Team totals as detection scope | Leave out; team-total IDs are only used where useful for rescue key handling. |
| Fourth-book or generic multi-book expansion | Leave out. |
| Production stake profile migration | Leave out until low-stake live tests are stable. |
