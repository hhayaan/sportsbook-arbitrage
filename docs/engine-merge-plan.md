# Engine Merge Plan

This file tracks the merge from the partner Hunter engine into our Rust engine.
Hunter is the empirical behavior source. Our project remains the target runtime:
pure Rust engine logic, terminal-first operation, and current market scope only.

## Guardrails

- Keep the engine pure Rust. Browser/API automation stays in `bet-placer` as an
  adapter bridge only.
- Keep the operator surface in terminal output and existing logs. Do not port a
  GUI or frontend.
- Keep current market scope to moneyline, totals, and spreads.
- Do not port player props in this wave.
- Optimize for live latency. Extra safety checks must be bounded by short
  timeouts and must not add a second Pinnacle browser.
- Dedup / family / ladder behavior is the only planned second-phase merge item.

## Difference Inventory

| Area | Our Rust engine before merge | Hunter engine | Current merge status |
|---|---|---|---|
| Runtime role | Rust was a maintainable engine foundation, but not equivalent to Hunter's latest production behavior. | `hunter.js` is the partner project's production runtime and carries recent incident fixes. | Hunter remains the behavior spec; Rust remains our runtime. |
| Implementation shape | Typed Rust modules with clearer boundaries. | Large JavaScript monolith with many coupled state maps. | Keep Rust modules; do not port the monolith. |
| Core model | Canonical order book using `CanonicalMarket`, `MarketIdentityKey`, and generic cross-venue detection. | Tactical DK/FD soft book against Pinnacle hedge scanner. | Keep Rust's canonical model. Main live loop still requires a Pinnacle leg. |
| Detection scope | Can theoretically detect DK/FD/PIN combinations, but production path already requires Pinnacle. | Production fire path is soft book first, Pinnacle hedge second. | Kept. No DK-vs-FD live expansion. |
| Market scope | Moneyline, spread, total fields, plus model capacity for props/other surfaces. | Broader canary surfaces, player props, alts, and period experiments. | Keep ML/spreads/totals only. Props and broad surfaces are unplanned. |
| Public Pinnacle odds | Public/book Pinnacle prices could influence candidate math before live execution. | Public/guest PIN is discovery only; logged-in quote is executable truth. | Merged for execution. |
| Authenticated Pinnacle quote | Existing `quote_bet` was present but not fully binding for stake math. | Logged-in quote price/min/max is the hedge truth. | Merged. |
| Quote freshness model | Book freshness and odds drift checks existed, but not Hunter's exact quote freshness discipline. | Discovery cache, authenticated quote, explicit final recheck. | Merged on execution path: initial logged-in quote plus final pre-fire recheck. |
| FD soft validation | No authenticated FD pre-fire refresh in live execution. | Authenticated FD market-price refresh before soft-leg risk. | Merged core behavior via `validate_soft` using the logged-in FD adapter. |
| DK soft validation | No `validate_soft` pre-fire DK calculate pass. | DK calculate/prepare validation catches stale CDN/feed prices. | Merged core authenticated price check. Hunter's NewOffer filled-leg handoff is not fully ported. |
| PIN pre-fire recheck | Only initial pre-flight quote. | Re-quotes PIN immediately before `LEG1_SENT`; offline/arb-dead aborts, timeout can fail open. | Merged with `PIN_PREFIRE_RECHECK_TIMEOUT_MS`, default 1500ms. |
| PIN hard-yank blacklist | No temporary memory for PIN 410/404/OFFLINE. | TTL blacklist by matchup/period/family plus umbrella cooldowns. | Partially merged as a market-id TTL blacklist. Family/umbrella behavior is not merged. |
| Execution ordering | Soft book first and Pinnacle second. | Soft-first only after quote, soft validation, and PIN pre-fire checks. | Merged for Pinnacle-backed live execution. |
| Attempt lifecycle | Mostly implicit in tracing and return paths. | Structured attempt lifecycle stages. | Merged as JSONL event files while keeping terminal output. |
| Post-fill hedge math | Could rely on planned stake/odds farther into the path. | Recompute hedge from actual filled soft stake and odds. | Merged. |
| Post-rounding profitability | Stake math was less isolated from quote bounds. | Uses quote min/max and rounded stake viability before firing. | Merged in `execution_math` and live executor checks. |
| Rescue sizing | Rescue module previously used profit-based sizing. | Rescue stake balances payouts: `leg1_stake * leg1_odds / rescue_odds`. | Merged. |
| Adjacent-line rescue | No live PIN adjacent-line rescue path. | Tries original PIN line, then adjacent totals/spreads/team totals. | Merged for PIN market IDs. |
| N-miss PIN 410 rescue threshold | No rescue-specific hard-yank counter. | A single 410 does not kill the rescue family; default threshold is 3. | Merged with `RESCUE_PIN_HARD_YANK_THRESHOLD`, default 3. |
| Cashout comparison | No rescue-vs-cashout economics. | Skip a hedge when available cashout locks a smaller loss. | Merged when DK/FD adapter status exposes a cashout offer. |
| Cashout execution | No Rust recovery cashout flow. | Attempts cashout after failed rescue, then confirms status. | Merged for DK and FD adapter paths. |
| Cashout confirmation | No raw DK cashout confirmation state machine. | Raw `Cashed Out` is required for DK; FD `cashoutBet` success confirms FD cashout. | Merged for Rust classifier, DK bridge handling, and FD confirmed cashout result. |
| Raw DK status classification | No Rust classifier. | Open-first word-boundary classifier avoids the `Unsettled` trap. | Merged with tests. |
| Naked-leg persistence | No durable unresolved-exposure ledger. | Writes naked legs and reloads them after restart. | Merged as `data/naked-legs.json`. |
| Naked-leg monitor | No periodic recovery monitor. | Polls status/cashout and keeps exposure alive until resolved. | Merged in Rust polling loop. |
| PIN relist recovery | No relist watcher. | Recovery-only watcher quotes PIN again for existing naked legs. | Merged for original PIN market relist. |
| Settlement ingest | Simpler trade history/accounting. | Separate settlement ingest and MyBets fallback hardening. | Not merged. |
| Dedup/family/ladder | Basic time dedup only. | Family-scope dedup plus ladder allowlist. | Shelved for next merge phase. |
| Period identity | Rust has period in canonical identity. | Hunter preserves period suffixes in family/ladder keys and gates broad period surfaces. | Existing Rust identity retained; Hunter ladder period behavior shelved with dedup. |
| Telemetry | Tracing, SQLite records, dashboard events. | JSONL global events and per-attempt files. | Merged as `data/events.jsonl` and `data/bets/<attempt_id>.jsonl`. |
| Replay fixtures | Unit tests existed, no Hunter replay harness. | Docs require incident replay fixtures. | Not merged except focused unit tests. |
| Environment profile | Rust env/settings shape. | Heavy Hunter env profile. | Only feature-specific env gates were translated. |
| Frontend/UI | Terminal-first engine operation. | Partner repo has broader operational surfaces. | Not planned. |
| Player props | Out of current scope. | Hunter has prop scan/canary work. | Not planned. |
| Hunter weaknesses | Rust avoids monolith/state-map risks. | Hunter has state coupling, env dependence, and difficult-to-audit control flow. | Do not port those qualities. |

## Merged First Wave

### Authenticated Pinnacle Quote Truth

Files:

- `engine/src/execution_math.rs`
- `engine/src/live_executor.rs`
- `bet-placer/index.js`

What our engine previously did:

- It already called `quote_bet` for Pinnacle, but planned candidate odds and
  stake math could still leak into execution.

What was merged:

- `AuthenticatedQuote` validates quote price, min stake, and max stake.
- Live execution treats the logged-in Pinnacle quote as the binding hedge price.
- PIN stake is computed from the soft-side payout and the authenticated quote.

How this changes our process:

- Public Pinnacle odds remain discovery only.
- Before soft-leg risk, the engine checks the executable logged-in quote from
  the single logged-in Pinnacle browser/API context.

What was left out:

- Hunter's broader family-level PIN quote caches and matchup umbrella cooldowns.

Recommendation:

- Keep this. It is the foundation for every safe Pinnacle-backed fire.

### DK / FD Soft Pre-Fire Validation

Files:

- `engine/src/live_executor.rs`
- `bet-placer/index.js`

What our engine previously did:

- It trusted the scanned soft-book candidate price until placement.

What was merged:

- Rust sends `validate_soft` before soft-leg risk.
- DraftKings validation uses authenticated `calculateBets`.
- FanDuel validation uses authenticated `getSelectionInfo`, which refreshes
  market prices through the logged-in FD adapter.
- Rust uses the fresh soft price for sigma checks and downstream PIN stake math.

How this changes our process:

- A stale DK/FD candidate can be killed before the soft bet is sent.
- If the price is still live, the engine continues with the fresh soft odds.

What was left out:

- Hunter's full DK NewOffer / accepted-during-drift handoff semantics.
- Hunter's richer retry taxonomy for validation failures.

Recommendation:

- Keep this core validation now. Add NewOffer handoff only after live testing
  proves we need that edge case.

### PIN Pre-Fire Recheck

Files:

- `engine/src/live_executor.rs`
- `engine/src/pin_yank.rs`

What our engine previously did:

- It quoted Pinnacle once earlier in the execution path.

What was merged:

- Rust re-quotes Pinnacle immediately before `LEG1_SENT`.
- Default timeout is 1500ms via `PIN_PREFIRE_RECHECK_TIMEOUT_MS`.
- `410`, `404`, `OFFLINE`, unavailable, suspended, and similar hard-yank errors
  abort before soft risk and record a temporary PIN blacklist.
- Timeout or unclassified transient failure fails open, matching Hunter's
  latency/safety tradeoff.

How this changes our process:

- The last check before risking the soft leg asks whether Pinnacle is still
  hedgeable right now.
- It does not add another browser; it reuses the same logged-in Pinnacle quote
  route.

What was left out:

- Hunter's family-scope hard-yank key and game umbrella cooldown.

Recommendation:

- Keep the market-id blacklist for now. Revisit family-scope yank behavior when
  dedup/family keys are merged.

### Actual-Fill Hedge Math

Files:

- `engine/src/execution_math.rs`
- `engine/src/live_executor.rs`

What our engine previously did:

- It could carry planned soft stake/odds into the hedge decision.

What was merged:

- After leg 1 fills, Rust recomputes:

```text
pin_stake = soft_filled_stake * soft_filled_decimal_odds / pin_decimal_odds
```

- The recompute uses the authenticated PIN quote and checks max stake, sigma,
  and guaranteed profit before sending the hedge.

How this changes our process:

- Hedge size follows the bet that actually filled, not the planned bet.

What was left out:

- Nothing material for this feature.

Recommendation:

- Keep this permanently.

### Recovery Stack

Files:

- `engine/src/live_executor.rs`
- `engine/src/naked_legs.rs`
- `engine/src/pin_yank.rs`
- `bet-placer/index.js`
- `bet-placer/src/sportsbooks/draftkings.js`
- `bet-placer/src/sportsbooks/fanduel.js`

What our engine previously did:

- If leg 1 filled and leg 2 failed or post-fill math killed the arb, execution
  usually logged the exposure and returned `None`.

What was merged:

- Adjacent-line PIN rescue for moneyline, total, spread, and team-total PIN key
  shapes. Moneyline retries original only; totals/spreads try Hunter-style
  neighboring lines.
- Rescue stake uses payout balancing.
- Rescue-specific N-miss hard-yank threshold defaults to 3.
- If DK status exposes a cashout offer and cashout is cheaper than the rescue
  loss, Rust skips the hedge and attempts cashout instead.
- If rescue and cashout do not resolve exposure, Rust writes a durable naked-leg
  entry to `data/naked-legs.json`.
- The polling loop periodically reloads naked legs, attempts original-line PIN
  relist recovery, checks soft-book status, and retries cashout when available.

How this changes our process:

- A failed Pinnacle hedge is no longer a dead-end log line.
- Restarting the engine does not erase unresolved exposure.

What was left out:

- Full Hunter cashout retry schedule at 30s/60s/120s/300s/600s.
- DK MyBets websocket/history fallback.
- Hunter's broader settlement-ingest pipeline.

Recommendation:

- Keep this first-wave recovery path. Treat DK live testing as mandatory before
  raising stake size.

### DK Cashout / Status Classifier

Files:

- `engine/src/status_classifier.rs`
- `bet-placer/src/sportsbooks/draftkings.js`

What our engine previously did:

- It lacked a Rust equivalent of Hunter's raw status rules.

What was merged:

- `Unsettled`, `Not Settled`, `Open`, `Pending`, `Placed`, `Accepted`,
  `Processing`, and `LiveDelay` remain open.
- `Cashed Out` is distinct from ordinary settlement.
- `Settled`, won/lost, voided, refunded, canceled, and push are terminal
  non-cashout outcomes.
- DK `getBetStatus` and `cashOut` bridge methods return raw status fields for
  Rust classification.

How this changes our process:

- We do not trust a normalized "settled" status if raw DK status says the bet is
  still open.
- Cashout is only treated as resolved when raw status supports it or the DK
  bridge returns confirmed cashout.

What was left out:

- Settlement P&L ingestion and MyBets history fallback.

Recommendation:

- Keep this. Add settlement ingest later as an accounting feature, not as part
  of dedup/ladder.

### FD Cashout

Files:

- `bet-placer/src/sportsbooks/fanduel.js`
- `bet-placer/index.js`
- `engine/src/live_executor.rs`

What our engine previously did:

- The FanDuel adapter could place bets and refresh market prices, but it did
  not expose open bets, cashout tokens, or a cashout method.
- FD placement results only carried bet IDs inside nested bet data, which made
  rescue/cashout paths less reliable.
- The adapter read the older `location_token.jwt` geo source and did not have a
  cashout write path with Hunter's `x-geo-packet` freshness rule.

What was merged:

- `getOpenBets()` calls FD's HAR-verified
  `getEligibleBets?sortBy=START_TIME` endpoint and extracts open bet IDs,
  status, cashout offer, `cashOutToken`, and bet delay.
- `cashOut(betId)` fetches a fresh `cashOutToken` immediately before sending
  FD's `cashoutBet` request.
- FD `executeBet()` now exposes top-level `_betId` and `_purchaseId`, mirroring
  Hunter's fix so Rust receives `bet_id` / `purchase_id` in the fill report.
- The adapter now reads `localStorage.geolocation_document.jwt` first, with
  `location_token.jwt` only as a legacy fallback.
- FD cashout writes use a browser-page POST path instead of Node-side
  `context.request`, and the path decodes the geo JWT expiry and refreshes it
  only when missing or near expiry.
- A Rust-bridge `getBetStatus()` wrapper was added around `getOpenBets()` so
  existing rescue/cashout comparison can see FD cashout offers.
- Successful FD cashout returns `confirmed: true` and raw status `Cashed Out`
  so Rust marks the exposure resolved.

How this changes our process:

- If FanDuel is leg 1 and the Pinnacle hedge fails, Rust can now compare a live
  FD cashout offer against rescue loss, then cash out FD when appropriate.
- If no cashout offer was known before rescue, the final cashout attempt still
  fetches a fresh FD token before writing.
- This does not add a second FD browser or a GUI surface. It uses the existing
  logged-in FD browser context.
- Fresh geo-token refresh only happens when the JWT is missing or stale, so a
  fresh session does not pay the extra navigation cost on every cashout.

What was left out:

- Hunter's `node-tls-client` / TLS impersonation transport.
- Hunter's FD UI fallback placement machinery.
- Hunter's NBA player-prop, alt-market, and partial-period FD code.
- Hunter's large FD diagnostics/event-logger/lifecycle stack.
- Hunter's DSC-0015 rolling hard-reset counter.
- Hunter's settlement-ingest and standalone open-bets polling subsystem.
- Hunter's manual FD cashout test scripts as runtime code.
- A full replacement of our compact FD adapter with Hunter's much larger
  FanDuel adapter.

Recommendation:

- Keep this scoped port. If FD cashout still hits DSC-0015 in live testing,
  consider adding Hunter's hard-reset recovery before considering TLS
  impersonation or the broader FD adapter.

### Structured Attempt Telemetry

Files:

- `engine/src/attempt_telemetry.rs`
- `engine/src/live_executor.rs`

What our engine previously did:

- It relied on terminal/tracing output and SQLite trade records.

What was merged:

- Rust writes global JSONL events to `data/events.jsonl`.
- Rust writes each attempt lifecycle to `data/bets/<attempt_id>.jsonl`.
- Events include quote, validation, pre-fire recheck, leg sent/filled, rescue,
  cashout, naked-leg, and monitor stages.

How this changes our process:

- Terminal output remains primary for the operator.
- After an incident, we can read one attempt file and see the full lifecycle.

What was left out:

- Hunter's exact event schema and `event-logger.js`.

Recommendation:

- Keep the Rust-native schema and refine it from real test runs.

### Test And Fixture Hygiene

Files:

- `engine/src/execution_math.rs`
- `engine/src/status_classifier.rs`
- `engine/src/live_executor.rs`
- `engine/src/odds_verifier.rs`
- `engine/src/rescue_hedger.rs`
- `engine/src/team_names.rs`

What our engine previously did:

- Some tests were stale after model changes.

What was merged:

- Tests now cover authenticated hedge math, DK raw status classification, and
  PIN adjacent-line rescue ID expansion.
- Stale test constructors were updated to current model fields.

What was left out:

- Full Hunter incident replay fixture harness.

Recommendation:

- Add replay fixtures after the first live test cycle exposes which cases are
  most valuable to freeze.

## Shelved For Next Merge Phase

### Advanced Family / Ladder Dedup

This is the only feature currently shelved for the second merge phase.

What Hunter has:

- Family-scope dedup so the same event/market family cannot refire repeatedly
  just because lines churn.
- Ladder controls that allow repeated adjacent-line firing only for explicit
  allowlisted families.
- Period-aware family keys so full game, halves, quarters, or periods do not
  accidentally block or allow each other.
- Telemetry around ladder allowed/blocked decisions.

What our engine has:

- Basic time-based trade dedup in the polling loop.

Why this is next:

- It directly controls repeated exposure.
- It can be ported without changing GUI, props, or market scope.
- It should reuse the same family identity work needed for future PIN
  family-level yank blacklists.

Recommended second-phase scope:

- Add a Rust family key for live fire dedup.
- Preserve period in the dedup key.
- Default to family-scope blocking.
- Add explicit ladder allowlist behavior only where configured.
- Keep terminal logs clear when a candidate is blocked or allowed by dedup.

## Not Merged And Not In The Next Merge Phase

| Feature / difference | Current status | Recommendation |
|---|---|---|
| GUI / frontend / dashboard port | Not merged and not planned. | Leave out entirely. |
| Player props | Not merged and not planned. | Leave out until ML/totals/spreads are stable. |
| Full Hunter monolith | Not merged and not desired. | Preserve typed Rust modules. |
| Hunter state-map coupling | Not merged and not desired. | Model state explicitly in Rust when needed. |
| DK NewOffer filled-leg handoff | Not merged. | Consider after first-wave testing if DK drift accepts create unknown/partial outcomes. |
| DK validation retry taxonomy | Not merged. | Add only if logs show transient validation failures worth retrying. |
| DK MyBets websocket/history fallback | Not merged. | Add with settlement/cashout hardening, not dedup. |
| Full cashout retry schedule | Not merged. | Add after observing whether immediate cashout misses are common. |
| FD TLS impersonation transport | Not merged. | Leave out unless page-context cashout still fails with evidence. |
| FD UI fallback placement | Not merged. | Leave out unless FD API placement becomes unreliable and we intentionally expand adapter scope. |
| FD DSC-0015 hard-reset counter | Not merged. | Add only if logs show repeated stale/invalid geo-token failures after the scoped refresh. |
| Settlement ingest parity | Not merged. | Add later for P&L/accounting correctness. |
| Family-level PIN hard-yank blacklist | Not merged. | Pair with dedup/family key work or a later safety phase. |
| Matchup umbrella PIN cooldown | Not merged. | Pair with family/yank design after testing market-id blacklist behavior. |
| Full Hunter event schema | Not merged. | Keep Rust-native telemetry unless an external tool requires Hunter parity. |
| Historical incident replay harness | Not merged. | Strongly recommended before expanding recovery/cashout complexity. |
| Hunter env profile compatibility | Not merged. | Translate only gates tied to features we actually port. |
| Broad period expansion | Not merged. | Leave gated until period identity and settlement proof are stronger. |
| FD basketball alt totals/spreads | Not merged. | Leave out until operator canary/replay exists. |
| FD NBA 1H / partial-period surfaces | Not merged. | Leave out until period identity is proven. |
| Team totals beyond rescue key support | Not merged as detection scope. | Keep current ML/totals/spreads fire scope. |
| Production stake profile migration | Not merged. | Do not raise stakes until first-wave live tests are reviewed. |
| Fourth-book / generic multi-book expansion | Not merged. | Leave out; this merge is safety into Rust, not sportsbook expansion. |
| Hunter canary/revenue unlocks | Not merged. | Leave out until the core live-money safety path is stable. |
