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
| FD soccer identity | FD soccer scan maps supported FanDuel competitions into the same league namespaces as Pinnacle and DK. | Hunter keeps league/event identity aligned before comparing books. | Phase-two partial merged for supported soccer leagues. |
| Final Pinnacle recheck | Short pre-fire quote before sending the soft leg. | Hunter rechecks PIN immediately before soft risk. | Merged and keep. |
| Stake sizing | `MAX_STAKE` caps only the soft-book stake in Pinnacle-backed arbs; Pinnacle hedge is quote, balance, and profit bounded. | Hunter sizes hedge from actual filled soft stake. | Merged and keep. |
| Pinnacle fill parsing | Node normalizes `purchase_id` and `bet_id` to strings; Rust also accepts numeric IDs. | Hunter's PIN IDs are numeric in accepted responses. | Merged after live timeout evidence. |
| DK status/cashout classifier | Rust has raw-status classification and DK bridge support. | Hunter fixed the `Unsettled` trap with raw open-first logic. | Merged and keep. |
| FD cashout | FD open-bet lookup, fresh `cashOutToken`, browser-context cashout write, and geo JWT freshness are implemented. | Hunter has a larger FD cashout stack. | Scoped core merged; extra FD stack is out unless evidence requires it. |
| Recovery | Original/adjacent Pinnacle rescue, cashout comparison, naked-leg persistence, monitor reload, and original-line relist recovery are wired. | Hunter has broader retry scheduling and settlement support. | First-wave core merged; retry schedule and settlement are not merged. |
| Attempt telemetry | JSONL global and per-attempt events are written under `engine/data` by default. | Hunter has structured lifecycle telemetry. | Rust-native telemetry merged. |
| Detector diagnostics | Optional JSONL detector diagnostics can count market availability, venue-pair funnel rejects, candidates, min-edge skips, and compact Pinnacle pair snapshots. | Hunter has richer forensics around why candidates did or did not fire. | Phase-two Rust-native diagnostics merged; no UI. |
| Dedup/family/ladder | Runtime now has persistent family-scope dedup keyed by sport, event, soft book, market type, and period. Successful two-leg fills allow limited re-entry; specific soft-book limit/unavailable-market failures hard-lock the family. | Hunter also has true ladder buckets, allowlists, and edge-improvement gates. | Phase-two core merged; ladder buckets and edge gates are left out for now. |
| Pinnacle period handling | Family dedup preserves period labels, unknown Pinnacle periods are skipped instead of becoming `full_game`, and non-full-game Pinnacle moneylines are dropped at parse time. | Hunter preserves period suffixes in family/ladder keys and treats period mapping as a binding safety invariant. | Phase-two safety fix merged after the `s;10;m` MLB live-moneyline incident. |
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
- Revisit family-level yank behavior only if exact-market blacklisting leaves repeated same-family Pinnacle failures in live evidence.

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

## Merged Phase Two

### Family-Scoped Dedup And Soft-Limit Lock

This is the first phase-two merge item. It replaces the old short in-memory event/market/venue-pair cooldown with persistent family-level state.

What our engine previously did:

- The main loop kept an in-memory 60-second cooldown.
- The key was effectively `event_id + market_type + leg1_venue + leg2_venue`.
- It only locked after a successful trade.
- It was order-sensitive, short-lived, not persisted, and did not stop repeated same-event attacks after a soft-book exposure limit rejection.

Current behavior:

- Rust writes family dedup state to `data/family-dedup.json` by default.
- The default family TTL is 6 hours.
- Successful two-leg fills do not hard-lock the family by default.
- A successful two-leg fill records the family with reason `both_filled`, starts a 30-second family cooldown, and increments the successful fill count.
- After the cooldown, the same family may fire again until it reaches the configured repeat limit.
- The default success repeat limit is 3 repeats after the first fill, meaning up to 4 successful fills per family TTL window.
- The family key is `sport + event_id + soft_venue + market_type + period`.
- The key is order-independent for Pinnacle/soft-book pairs, so DK/PIN and PIN/DK are treated as the same family.
- Period is preserved in the key.
- Line and side are intentionally not in the key, so repeated attempts on nearby lines in the same event/market/soft-book family share the same cooldown/repeat state.
- Before live execution, Rust checks the family lock locally before spending time on quote, validation, or placement.
- A success cooldown produces `policy=success_cooldown`.
- Hitting the success repeat limit produces `policy=success_repeat_limit`.
- Specific soft leg-1 failures hard-lock the family for the TTL: `MaxExposureReached`, `CustomerLimitsError`, explicit stake/limit errors, and market/selection closed, suspended, or unavailable errors.
- Generic `PurchaseNotAccepted`, generic `declined`, slippage, auth/session/browser errors, network errors, stale-ID market-not-found errors, insufficient funds, Pinnacle quote failures, and Pinnacle placement failures do not hard-lock the family.
- Terminal output prints `[FAMILY DEDUP] Skip ...`, `[FAMILY DEDUP] Hard-locked ...`, and `[FAMILY DEDUP] Recorded successful fill ...`.
- JSONL telemetry emits `FAMILY_DEDUP_SKIP`, `FAMILY_DEDUP_LOCKED`, and `FAMILY_DEDUP_SUCCESS_RECORDED`.

Configuration:

- `FAMILY_DEDUP_FILE` overrides the lock file path.
- `FAMILY_DEDUP_TTL_MS` overrides the lock TTL in milliseconds.
- `FAMILY_DEDUP_TTL_SECS` overrides the lock TTL in seconds.
- `FAMILY_DEDUP_SUCCESS_COOLDOWN_MS` overrides the successful-fill cooldown in milliseconds.
- `FAMILY_DEDUP_SUCCESS_COOLDOWN_SECS` overrides the successful-fill cooldown in seconds.
- `FAMILY_DEDUP_SUCCESS_MAX_REPEATS` overrides the number of allowed successful repeats after the first fill.
- `DEDUP_COOLDOWN_MS` and `DEDUP_COOLDOWN_SECS` remain accepted as compatibility aliases.

How it changes the process:

- After a trade fills on an event/market/soft-book family, the engine waits for the success cooldown before allowing another same-family attempt.
- The engine can place the same profitable family more than once, but only up to the configured repeat count.
- After DK/FD clearly says the account/selection/market is limit-blocked or unavailable on the first leg, the engine stops hammering that same family for the TTL.
- This preserves controlled re-entry while preventing the old behavior where one event could generate unbounded repeated DK/PIN attempts until DK returned exposure errors.
- Paper/scan runs do not write persistent family locks.
- Latency cost is local file-backed state lookup only. No new sportsbook request is added to the hot path.

What was left out:

- Hunter's true ladder bucket system.
- Hunter's ladder allowlist for configured alternate-line families.
- Hunter's family edge-improvement gate that can allow a later same-family attempt only when the edge meaningfully improves.
- Hunter's unknown-write family freeze. This project intentionally does not use manual/unknown fill status as a live placement outcome.
- Player props, broad alternate markets, expanded period surfaces, and frontend/UI dedup controls.
- Family-level Pinnacle yank umbrella cooldowns. Current Pinnacle hard-yank handling remains exact-market scoped.

Recommendation:

- Keep the family key broad while low-stake testing continues.
- Tune the 30-second success cooldown and 3-repeat limit from live evidence before adding more machinery.
- Add true ladder buckets only if we deliberately start trading multiple lines in the same event/market family.
- Add an edge-improvement gate only if repeat-limited re-entry is still too blunt.
- Keep unknown-write/manual reconciliation behavior out.

### FD Soccer League Identity And Detector Diagnostics

This is the second phase-two merge item. It addresses the evidence that FD/PIN detection worked for some MLB/NHL/NBA markets, but FD soccer could not match Pinnacle soccer because the two sources were using different sport namespaces.

What our engine previously did:

- FanDuel soccer was fetched once as generic `SOCCER`.
- Pinnacle and DraftKings soccer were fetched as league-specific sports such as `MLS`, `EPL`, `LA_LIGA`, `SERIE_A`, `BUNDESLIGA`, `LIGUE_1`, and `UCL`.
- The canonical event ID includes the sport prefix, so an FD event like `SOCCER:...:TOR-vs-NYC` could not share an identity key with Pinnacle `MLS:...:TOR-vs-NYC`.
- The detector mostly produced terminal candidates or nothing. It did not explain where FD/PIN opportunities were dying in the matching funnel.

Current behavior:

- FanDuel soccer now reads event competition metadata before building canonical event IDs.
- Supported FanDuel soccer competitions map to the configured league namespace:
  - `US MLS`, `Major League Soccer`, `MLS` -> `MLS`;
  - `English Premier League`, `England Premier League`, `Premier League` -> `EPL`;
  - `Spanish La Liga`, `Spain La Liga`, `La Liga` -> `LA_LIGA`;
  - `Italian Serie A`, `Serie A` -> `SERIE_A`;
  - `German Bundesliga`, `Bundesliga` -> `BUNDESLIGA`;
  - `French Ligue 1`, `Ligue 1` -> `LIGUE_1`;
  - `UEFA Champions League`, `Champions League` -> `UCL`.
- Unsupported FD soccer competitions are skipped instead of being compared under generic `SOCCER`.
- `PriceEntry` carries `venue_event_id` so snapshots can show the venue-side event identity behind a canonical match.
- Detector telemetry is optional and generic across venue pairs, not FD-only.
- With `DETECTOR_TELEMETRY=1` or `DETECTOR_DIAGNOSTICS=1`, Rust emits:
  - `DETECTOR_MARKET_COUNTS`;
  - `DETECTOR_FUNNEL_SUMMARY`;
  - `DETECTOR_PIN_PAIR_SNAPSHOT`;
  - `DETECTOR_CANDIDATE`;
  - `DETECTOR_CANDIDATE_SUMMARY`;
  - `MIN_EDGE_SKIPPED`;
  - `FAMILY_DEDUP_PASSED` / `FAMILY_DEDUP_SKIP`.
- `FIRE_ENTER` telemetry now includes venue pair and soft venue fields, making FD/PIN and DK/PIN attempts easier to filter.

How it changes the process:

- Supported FD soccer leagues can now share identity keys with Pinnacle and DK league feeds.
- FD/PIN investigations no longer have to guess whether the miss happened at source availability, identity matching, complementary selection pairing, sigma, min-edge, or live fire.
- Diagnostics are JSONL-only and gated by environment variable, so normal terminal output and hot-path latency stay lean.

What was left out:

- Hunter's frontend/operator telemetry views.
- Hunter's full event schema.
- FD authenticated scan overlay; Rust still scans public FD odds and validates authenticated FD price only before fire.
- FD player props, basketball alts, partial-period expansion, and broad alternate markets.
- True ladder buckets and edge-improvement gates beyond the current broad family lock.
- Any loosening of the three-way soccer moneyline block.

Recommendation:

- Run at least one scan with `DETECTOR_TELEMETRY=1` before changing non-soccer normalization.
- Use the venue-pair summaries and snapshots to decide whether FD misses are from no shared identity, no complementary selections, sigma, min edge, family dedup, or live validation.
- Consider FD authenticated scan overlay only if diagnostics and live validation logs show public FD scan prices are stale often enough to matter.

## Required Fixes Before Higher-Stake Live Use

These are current project work items that should be addressed before raising stake size.

| Work item | Reason |
| --- | --- |
| Improve recovery terminal visibility | Cashout attempts/results are currently visible in JSONL telemetry, but not always obvious in terminal output. |
| Add incident replay fixtures | Live issues should become deterministic tests before recovery logic grows. |
| Review detector telemetry from FD/PIN runs | The new diagnostics should be used before changing non-soccer normalization or FD authenticated scan behavior. |

## Not Merged And Not Planned

| Feature | Decision |
| --- | --- |
| GUI/frontend port | Leave out. |
| Player props | Leave out until ML/totals/spreads are stable. |
| Full Hunter monolith | Leave out. |
| Hunter indeterminate Pinnacle write response | Leave out. Do not send `fill_report status="unknown"` / `retry_allowed=false` as a live placement outcome. The engine must receive automated filled/rejected/error truth. |
| Manual unknown-fill reconciliation status | Leave out. The engine must not require operator reconciliation as part of normal automation. |
| Hunter true ladder buckets | Leave out for now. Revisit only if the engine intentionally trades multiple lines in the same event/market family. |
| Hunter ladder allowlists | Leave out for now with true ladder buckets. |
| Hunter family edge-improvement gate | Not planned yet. Revisit only if broad family locks become too conservative in live testing. |
| Hunter unknown-write family freeze | Leave out because unknown write status is not a valid live placement outcome in this engine. |
| Second Pinnacle browser | Leave out. |
| Guest/auth Pinnacle double-scan | Leave out unless explicitly reintroduced for diagnostics. |
| FD TLS impersonation | Leave out unless browser-context writes fail with evidence. |
| FD UI fallback placement | Leave out unless API placement becomes unreliable with evidence. |
| FD authenticated scan overlay | Leave out unless detector telemetry and validation logs prove public FD scan prices are a recurring cause of missed or dead FD arbs. |
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
