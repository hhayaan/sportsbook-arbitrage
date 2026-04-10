# Codebase Concerns

**Analysis Date:** 2026-04-09

## Tech Debt

**Rescue Hedger Exists But Is Never Called:**
- Issue: `rescue_hedger.rs` computes a rescue plan when leg 2 fails, but `live_executor.rs` never calls `compute_rescue`. On leg 2 failure the engine just logs "LEG 1 PLACED, EXPOSED!" and returns `None`. The entire rescue workflow is dead code in production.
- Files: `engine/src/rescue_hedger.rs`, `engine/src/live_executor.rs` (lines 483–494)
- Impact: Any leg 2 failure leaves a live unhedged bet with no automated mitigation. Requires manual intervention to limit losses.
- Fix approach: After leg 2 fails (the two `tracing::error!` branches at lines 484 and 492), call `compute_rescue` with the book snapshot and wire the resulting `RescuePlan` into a third `place_leg` call.

**State Machine and Policy Engine Are Unused:**
- Issue: `state_machine.rs` (174 lines, defines `ArbAttempt` with 19 states) and `policy.rs` (109 lines, defines `check_gates`) are never invoked from `main.rs` or `live_executor.rs`. The execution path goes directly from detection to `live_executor::try_execute`, bypassing both modules entirely.
- Files: `engine/src/state_machine.rs`, `engine/src/policy.rs`, `engine/src/trades.rs`
- Impact: State machine transitions are never recorded; structured trade logging in `trades.rs` (`log_arb_detected`, `log_execution_closed`, etc.) is also never called. Policy gates in `policy.rs` duplicate logic already inline in `main.rs` but do not replace it.
- Fix approach: Either integrate `ArbAttempt` into `live_executor::try_execute` to replace the current procedural flow, or delete these modules to reduce maintenance surface.

**Treasury Module Is Unused:**
- Issue: `treasury.rs` (123 lines) implements a full reserve/commit/fill/release capital accounting system with in-flight tracking, but is never instantiated or called. `main.rs` only declares `mod treasury;`.
- Files: `engine/src/treasury.rs`, `engine/src/main.rs` (line 16)
- Impact: Capital is not reserved before bets are placed. If two arbs fire simultaneously for the same venue, both could proceed with the same balance, potentially overdrawing a book.
- Fix approach: Instantiate `Treasury` in `main.rs`, call `reserve` before each execution attempt, `commit` on leg 1 submission, `fill` on confirmation, and `release` on failure.

**DK API Version Hardcoded in Headers:**
- Issue: `x-client-version: "2612.4.1.6"` and `x-client-widget-version: "2608.3.1"` are hardcoded constants in `bet-placer/src/sportsbooks/draftkings.js` (lines 17–24). These version strings are read and validated by the DK API.
- Files: `bet-placer/src/sportsbooks/draftkings.js`
- Impact: When DK deploys a new frontend version these version strings go stale, causing bets to be rejected with NewOffer errors.
- Fix approach: Extract these to a config constant with a comment noting the last verified version and date. Consider detecting version from the loaded DK page at startup.

**FD Balance Scraping Is Fragile:**
- Issue: `getBalance()` in `fanduel.js` (lines 292–325) finds the balance by iterating every `span, div, p, button` on the page and taking the largest `$X` match. It has no scope constraint and will break if FD adds any other dollar amount to the page layout.
- Files: `bet-placer/src/sportsbooks/fanduel.js` (lines 292–325)
- Impact: Silent wrong balance readings; over- or under-reports available stake, causing incorrect stake scaling or missed trades.
- Fix approach: Use the FD account API endpoint directly (similar to DK's `dashes/v1/dashes/siteNav/users/me.json`) rather than DOM scraping.

**`require_pinnacle_leg` Is a Hardcoded True Constant:**
- Issue: In `main.rs` line 228, `let require_pinnacle_leg = true;` is hardcoded as a local variable rather than wired to settings or an env var. There is no runtime way to disable the Pinnacle requirement without recompiling.
- Files: `engine/src/main.rs` (line 228)
- Impact: Cannot test DK-FD arb detection without a code change.
- Fix approach: Add a `REQUIRE_PINNACLE` env var similar to `SKIP_LIVE`.

## Known Bugs

**Simultaneous Balance Overdraw Risk:**
- Symptoms: Two arb candidates involving the same venue fire in the same polling cycle; both pass the balance check (reading the same DB value), and both proceed to place bets.
- Files: `engine/src/main.rs` (lines 236–382), `engine/src/db.rs` (`get_balance`)
- Trigger: Multiple arbs detected in the same 5-second cycle with overlapping venues.
- Workaround: The 60-second dedup cooldown prevents repeated trades on the same market, but does not prevent two different markets at the same venue from firing simultaneously.

**Post-Fill Sigma Check Uses Stale Second Leg Odds:**
- Symptoms: Post-fill sigma revalidation at `live_executor.rs` line 463 computes `1.0 / o1 + 1.0 / second_odds` where `second_odds` is from the original detection snapshot, not re-read from the current book.
- Files: `engine/src/live_executor.rs` (lines 458–469)
- Trigger: Odds move significantly on the second venue between leg 1 placement and the check.
- Workaround: The check catches the worst case (leg 1 fill odds changed); drift in second leg odds is caught by the pre-execution `odds_verifier::verify_both_legs` call, which runs before any bets.

**Dedup Key Does Not Include Selection Direction:**
- Symptoms: A FD-PIN arb on "Team A" (FD) vs "Team B" (PIN) for the same game/market would share the same dedup key as the reverse candidate "Team B" (FD) vs "Team A" (PIN), since the key is `event_id:market_type:leg1_venue:leg2_venue` with no selection.
- Files: `engine/src/main.rs` (lines 339–343)
- Trigger: Rare — most detection picks the same side — but could suppress a valid reverse arb.
- Workaround: None currently.

## Security Considerations

**TLS Certificate Validation Disabled Engine-Wide:**
- Risk: `danger_accept_invalid_certs(true)` in `main.rs` line 96 disables certificate verification for ALL outgoing HTTP requests from the Rust engine — not just DK. This includes FanDuel and Pinnacle odds fetches.
- Files: `engine/src/main.rs` (lines 94–98)
- Current mitigation: Traffic is odds-only (read), not auth-bearing from the engine side. Auth is handled via the browser in bet-placer.
- Recommendations: Use a separate `reqwest::Client` for DK (with certs disabled) and a standard client for FD and PIN. The DK proxy already exists specifically to avoid this; the engine should route all DK traffic through the proxy and use a verified client for other venues.

**Static API Keys Committed to Source:**
- Risk: Pinnacle `X_API_KEY = "CmX2KcMrXuFmNg6YFbmTxE0y9CIrOi0R"` is hardcoded in `bet-placer/src/sportsbooks/pinnacle.js` line 4. FanDuel `X_APPLICATION = "FhMFpcPWXMeyZxOx"` and `FD_API_KEY` are committed to `bet-placer/src/sportsbooks/fanduel.js` and `engine/src/config.rs`. These appear to be public/mobile app keys rather than user credentials.
- Files: `bet-placer/src/sportsbooks/pinnacle.js`, `bet-placer/src/sportsbooks/fanduel.js`, `engine/src/config.rs` (line 130)
- Current mitigation: These are likely public client-side keys tied to sportsbook apps, not user secrets.
- Recommendations: Confirm these are not rate-limited per-key. If they are, rotating them requires a code change; move to env vars.

**REST API Has No Authentication:**
- Risk: The axum HTTP server on port 3401 exposes settings mutation (`POST /api/settings`) and full reset (`POST /api/reset`) endpoints with a CORS policy of `allow_origin(Any)`. Any process on the machine can modify stake sizes or wipe trade history.
- Files: `engine/src/main.rs` (lines 418–435), `engine/src/api.rs`
- Current mitigation: Localhost only — not exposed to internet. Acceptable for single-user local deployment.
- Recommendations: Add a `localhost`-only bind if the machine is shared, or add a token header check on mutation endpoints.

## Performance Bottlenecks

**SQLite Mutex Contention on Every Poll Cycle:**
- Problem: `db.rs` uses a single `Mutex<Connection>` with no WAL mode or connection pool. Every `insert_arb`, `get_settings`, `get_balance`, and `update_balance` call blocks the mutex. A poll cycle with 20+ arb candidates will serialize all DB writes.
- Files: `engine/src/db.rs`
- Cause: `rusqlite` with a single shared connection and default journal mode.
- Improvement path: Enable WAL mode (`PRAGMA journal_mode=WAL`) on open, which allows concurrent reads with writes. For heavier write load, consider a per-write-transaction batch.

**Odds Fetcher Spawns One Task Per Sport Per Venue:**
- Problem: With 14 sports across 3 venues and multiple Pinnacle league IDs per sport, the fetcher spawns 50+ Tokio tasks every 5 seconds. Each task holds a clone of the `reqwest::Client`. There is no concurrency cap.
- Files: `engine/src/odds_fetcher.rs` (lines 52–130)
- Cause: Unbounded `tokio::spawn` in the fetch loop.
- Improvement path: Add a `tokio::sync::Semaphore` to cap concurrent outbound HTTP connections, preventing connection pool exhaustion during slow venue responses.

## Fragile Areas

**Team Name Normalization as a Static Lookup Table:**
- Files: `engine/src/team_names.rs`
- Why fragile: The 554-line file is a hand-maintained static map of team name variants. Any new team (expansion, relocation), name change, or venue-specific spelling variant not in the map will cause cross-venue event IDs to differ, making arbs invisible to the detector.
- Safe modification: Add new entries to `TEAM_MAP` in `engine/src/team_names.rs`. Do not remove existing entries — venues may use legacy names for historical events.
- Test coverage: No tests for `canonical_event_id` with cross-venue matching scenarios.

**detect_period String Matching in odds_fetcher.rs:**
- Files: `engine/src/odds_fetcher.rs` (lines 13–38)
- Why fragile: Period detection uses hardcoded substring matching on market label text. Venues can rename market labels at any time (e.g., "1st Qtr" vs "1st Quarter"). An unrecognized label silently falls through to `"full_game"`, causing a partial-period market to be treated as a full-game market and potentially match against a full-game market at another venue.
- Safe modification: Test any new period label patterns against both DK and FD naming conventions before deploying.
- Test coverage: No tests for `detect_period`.

**WebSocket Message Dispatch Is Single-Connection, No Reconnect:**
- Files: `engine/src/main.rs` (lines 104–136), `engine/src/live_executor.rs` (`connect`, `place_leg`)
- Why fragile: The WS connection to bet-placer is established once at startup. If bet-placer crashes and restarts, the engine continues running in paper mode silently (the `ws_conn` becomes `None` on the first WS error but the engine does not attempt reconnection). There is no periodic heartbeat or reconnect loop.
- Safe modification: Do not restart bet-placer during a live trading session without also restarting the engine.
- Test coverage: None for WS reconnection behavior.

**FanDuel Session Expires Every ~13 Minutes:**
- Files: `bet-placer/index.js` (lines 65–75)
- Why fragile: The keepalive timer refreshes all three browsers every 5 minutes via `page.reload()`. If a page reload takes longer than expected (slow navigation, PerimeterX challenge), the session can expire mid-execution, causing a `place_bet` failure as leg 1 or leg 2. The keepalive runs during active bet execution since there is no lock.
- Safe modification: The keepalive runs on a timer independent of the WS message handler; race conditions between a keepalive reload and an in-flight `executeBet` call are possible.
- Test coverage: None.

## Scaling Limits

**SQLite Single-File Database:**
- Current capacity: Adequate for current volume (tens of trades per day).
- Limit: Performance degrades with large `arb_opportunities` table (no index on `detected_at`). At 5-second polling intervals with many arb candidates, the table grows quickly. `get_arbs` with `LIMIT 100` is unindexed.
- Scaling path: Add `CREATE INDEX IF NOT EXISTS idx_arb_detected ON arb_opportunities(detected_at DESC)` in `migrate()`. For multi-machine or high-frequency deployment, migrate to PostgreSQL.

## Dependencies at Risk

**Patchright (Playwright Fork) for Browser Automation:**
- Risk: `bet-placer` depends on Patchright, a stealth fork of Playwright. It is not an official package and may lag behind Playwright's browser engine updates. DK/FD/PIN may deploy bot-detection updates that Patchright no longer bypasses.
- Impact: Bet placement silently fails or gets blocked; positions may go unhedged.
- Migration plan: Monitor Patchright release cadence. Fallback is Playwright with stealth plugins, or migrating DK to the proxy-based direct API approach already used for odds fetching.

## Missing Critical Features

**No Alerting on Unhedged Exposure:**
- Problem: When leg 2 fails after leg 1 fills, the only notification is a `tracing::error!` log line. There is no push alert (SMS, email, webhook) to notify the operator that manual intervention is required.
- Blocks: Operating the system unattended — a sleeping operator will not see a console log.

**No Actual P&L Tracking (Only Theoretical):**
- Problem: `theoretical_profit` is computed as `min(payout1, payout2) - cost` at the time of execution. There is no mechanism to record the actual settlement outcome after the event resolves. The `Stats` endpoint reports cumulative theoretical profit only.
- Blocks: Accurate performance measurement; detecting whether the 5% arb model holds in practice.

## Test Coverage Gaps

**No Tests for Live Execution Path:**
- What's not tested: `live_executor::try_execute` — the full two-leg placement sequence including quote, balance scaling, post-fill sigma check.
- Files: `engine/src/live_executor.rs`
- Risk: Stake sizing bugs, bad sigma revalidation logic, or WS message parsing errors could place wrong-sized bets or leave exposure undetected.
- Priority: High

**No Tests for Odds Fetcher Parsers:**
- What's not tested: `fetch_dk_inner`, `parse_fd_response`, `fetch_pinnacle` — the JSON → `CanonicalMarket` parsing pipelines.
- Files: `engine/src/odds_fetcher.rs`
- Risk: Venue API response format changes go undetected until live, resulting in missed markets or garbled event IDs.
- Priority: High

**No Tests for team_names::canonical_event_id Cross-Venue Matching:**
- What's not tested: Whether DK, FD, and PIN produce the same canonical event ID for the same game.
- Files: `engine/src/team_names.rs`
- Risk: Silent arb blindspots when a team name variant is missing from the lookup table.
- Priority: Medium

---

*Concerns audit: 2026-04-09*
