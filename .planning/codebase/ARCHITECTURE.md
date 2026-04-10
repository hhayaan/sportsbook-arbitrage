# Architecture

**Analysis Date:** 2026-04-09

## Pattern Overview

**Overall:** Multi-process pipeline with a Rust scanning engine and Node.js bet execution layer, connected via WebSocket. A separate Node.js HTTP proxy handles a Windows-specific TLS constraint for DraftKings.

**Key Characteristics:**
- Rust engine owns all decision logic: odds fetching, arb detection, safety filters, stake sizing
- Node.js layer owns browser automation: bet placement via Patchright stealth Chromium
- Cross-process communication via two WebSocket channels (engine→bet-placer for commands; engine exposes WS to clients for events)
- SQLite persists balances, trades, arb opportunities, and settings in `engine/data/trades.db`
- Polling loop runs every 5 seconds; diff-based book updates minimize redundant processing

## Layers

**Odds Ingestion:**
- Purpose: Fetch raw market data from all three venues and normalize to `CanonicalMarket`
- Location: `engine/src/odds_fetcher.rs`
- Contains: `fetch_all()` spawns concurrent tokio tasks per sport/venue; `fetch_draftkings`, `fetch_fanduel`, `fetch_fanduel_by_event_type`, `fetch_pinnacle` per-venue parsers; `detect_period()` normalizes market period labels; `american_to_decimal()` odds conversion
- Depends on: `engine/src/config.rs` (sport IDs, API hosts), `engine/src/team_names.rs` (team name normalization)
- Used by: `engine/src/main.rs` polling loop

**Order Book:**
- Purpose: In-memory store of current prices keyed by normalized market identity
- Location: `engine/src/models.rs` (`Book`, `MarketIdentityKey`, `VenueSelectionKey`, `PriceEntry`)
- Contains: `Book::update()`, `Book::touch()` (timestamp refresh without odds change), `Book::expire()` (30s staleness eviction), `Book::get_entries()`
- Depends on: Nothing (pure in-memory data structure)
- Used by: `engine/src/main.rs`, `engine/src/arb_detector.rs`, `engine/src/odds_verifier.rs`

**Arb Detection:**
- Purpose: Scan the book for cross-venue sigma < 1.0 opportunities
- Location: `engine/src/arb_detector.rs`
- Contains: `detect_arbs()` iterates all identity keys, pairs complementary selections across venues, applies sport/period/spread-sign safety filters, returns `Vec<ArbCandidate>` sorted by arb_pct descending
- Depends on: `engine/src/models.rs`
- Used by: `engine/src/main.rs` polling loop

**Safety Chain:**
- Purpose: Multi-layer gate between detected arb and placed bet
- Location: Distributed across `engine/src/circuit_breaker.rs`, `engine/src/rate_limiter.rs`, `engine/src/odds_verifier.rs`, with orchestration in `engine/src/main.rs`
- Contains: `CircuitBreaker` (loss+rejection sliding windows), `RateLimiter` (per-venue 1hr sliding window), `odds_verifier::verify_both_legs()` (5% drift check), dedup cooldown map (60s), SKIP_LIVE filter, time-to-start filter
- Depends on: `engine/src/models.rs`
- Used by: `engine/src/main.rs` polling loop (applied sequentially before execution)

**Stake Sizing:**
- Purpose: Compute optimal 2-leg stakes for guaranteed profit given capital and balance constraints
- Location: `engine/src/stake_optimizer.rs`
- Contains: `compute_stakes()` (basic), `compute_stakes_with_balances()` (balance-aware with scaling); Kelly-style proportional allocation using sigma
- Depends on: `engine/src/models.rs` (`ArbCandidate`, `StakePlan`)
- Used by: `engine/src/live_executor.rs`, `engine/src/paper_executor.rs`

**Execution:**
- Purpose: Place two-leg bets sequentially (soft book first, Pinnacle second)
- Location: `engine/src/live_executor.rs` (real), `engine/src/paper_executor.rs` (simulated)
- Contains: `try_execute()` (orchestrates quote→leg1→revalidate→leg2), `place_leg()`, `quote_leg()`, `fetch_balances()`, `drain_balance_updates()`; pre-flight Pinnacle quote + two-stage sigma revalidation
- Depends on: `engine/src/stake_optimizer.rs`, `engine/src/db.rs`, `engine/src/models.rs`
- Used by: `engine/src/main.rs` polling loop

**Bet Placer (Node.js):**
- Purpose: WebSocket server that translates engine commands into browser-automated sportsbook API calls
- Location: `bet-placer/index.js` (WS server + CLI), `bet-placer/src/browser-manager.js` (Patchright lifecycle), `bet-placer/src/sportsbooks/draftkings.js`, `bet-placer/src/sportsbooks/fanduel.js`, `bet-placer/src/sportsbooks/pinnacle.js`
- Contains: Message routing (`place_bet`, `quote_bet`, `get_balances`, `ping`), per-venue adapters with `executeBet()`, `getBalance()`, `getSelectionInfo()`, keepalive timer
- Depends on: Patchright (`chromium`), persistent Chrome profiles in `data/profiles/`
- Used by: `engine/src/live_executor.rs` via WebSocket

**Persistence:**
- Purpose: SQLite-backed store for balances, trades, arb log, settings
- Location: `engine/src/db.rs`
- Contains: `Db::open()` (auto-migrate + seed), CRUD for `book_balances`, `paper_trades`, `arb_opportunities`, `settings` tables; `get_stats()` for aggregate query
- Depends on: `rusqlite`
- Used by: `engine/src/main.rs`, `engine/src/live_executor.rs`, `engine/src/paper_executor.rs`, `engine/src/api.rs`

**REST/WS API:**
- Purpose: External control surface and real-time event stream
- Location: `engine/src/api.rs`, `engine/src/ws.rs`
- Contains: Axum routes for balances, trades, arbs, stats, settings; WS broadcaster using `tokio::sync::broadcast`; `AppState` shared between handlers
- Depends on: `engine/src/db.rs`, `engine/src/models.rs`
- Used by: External clients (HTTP on port 3401, WS on `/ws`)

**DK Proxy:**
- Purpose: Workaround for Rust `reqwest` TLS incompatibility with DraftKings CDN on Windows
- Location: `dk-proxy.js` (root)
- Contains: Single-route HTTP server; forwards `/dk/{leagueId}` to DK API using Node's native `fetch`
- Depends on: Nothing (stdlib only)
- Used by: `engine/src/odds_fetcher.rs` (primary DK fetch path; direct URL is fallback)

## Data Flow

**Odds Ingestion Flow:**

1. `main.rs` polling loop calls `odds_fetcher::fetch_all(client)` every 5s
2. `fetch_all` spawns concurrent tokio tasks: DK via proxy (port 3403), FD direct, PIN direct
3. Each fetcher parses raw JSON → `Vec<CanonicalMarket>` with normalized event_id, selection, decimal_odds, period, is_live
4. Main loop diffs against `market_hashes` — only changed entries call `book.update()`, unchanged call `book.touch()`
5. `book.expire()` evicts entries older than 30s

**Arb Detection and Safety Flow:**

1. `arb_detector::detect_arbs(&book, ...)` scans all `MarketIdentityKey` groups
2. Pairs complementary selections across different venues; applies 3-way block, period block, spread-sign check, Pinnacle requirement
3. Returns `Vec<ArbCandidate>` sorted descending by arb_pct
4. Main loop iterates candidates, persists each as `ArbOpportunity` in SQLite, broadcasts `WsEvent::ArbDetected`
5. Per candidate: circuit breaker → rate limiter → odds drift verify → HIGH% warn → SKIP_LIVE → time-to-start → dedup

**Execution Flow (Live Mode):**

1. `live_executor::try_execute()` called with validated `ArbCandidate`
2. Pinnacle pre-flight quote via WS (`quote_bet` → `quote_result`); quote sigma revalidated
3. Stake scaling via `stake_optimizer::compute_stakes_with_balances()`
4. Execution order: soft book (FD or DK) first, Pinnacle second
5. Leg 1 placed via `place_bet` WS command; `fill_report` awaited (20s timeout)
6. Post-fill sigma revalidated with actual accepted odds; abort if >= 1.0
7. Leg 2 placed; fill awaited
8. Trade persisted to SQLite; balances updated; `WsEvent::TradeExecuted` broadcast

**State Management:**
- In-memory: `Book` (order book), `CircuitBreaker` (sliding loss/rejection windows), `RateLimiter` (per-venue bet timestamps), `market_hashes` (diff state), `traded_keys` (dedup cooldown)
- Persistent: SQLite `engine/data/trades.db` — survives restarts

## Key Abstractions

**CanonicalMarket:**
- Purpose: Venue-agnostic normalized market representation
- Examples: `engine/src/models.rs:7`
- Pattern: All three venue fetchers output this struct; fields include normalized `event_id` (`SPORT:DATE:HOME-vs-AWAY`), `selection` (canonical team code or over/under), `market_type`, `period`, `decimal_odds`

**MarketIdentityKey:**
- Purpose: Composite hash key grouping same-market entries across venues for comparison
- Examples: `engine/src/models.rs:30`
- Pattern: `(event_id, market_type, line_key, period, is_live=false, player_key)` — `is_live` forced to false to allow cross-venue pairing where venues disagree on live status; spread `line_key` uses `abs()` to group both sides

**ArbCandidate:**
- Purpose: Detected 2-leg opportunity with sigma, arb_pct, venue/selection/odds for each leg
- Examples: `engine/src/models.rs:182`
- Pattern: Immutable snapshot from detection time; all downstream steps (safety, sizing, execution) read from this struct

**Adapter Pattern (Bet Placer):**
- Purpose: Common interface (`executeBet`, `getBalance`, `getSelectionInfo`) per venue despite completely different API mechanics
- Examples: `bet-placer/src/sportsbooks/draftkings.js`, `bet-placer/src/sportsbooks/fanduel.js`, `bet-placer/src/sportsbooks/pinnacle.js`
- Pattern: DK uses `page.evaluate(fetch())` with browser cookies; FD uses `context.request.post()`; PIN extracts tokens from `localStorage` and uses Node `fetch` directly

## Entry Points

**Engine:**
- Location: `engine/src/main.rs`
- Triggers: `cargo run` in `engine/`
- Responsibilities: Reads env vars, initializes DB, starts WS broadcaster, spawns polling loop as tokio task, starts Axum HTTP server on port 3401

**Bet Placer:**
- Location: `bet-placer/index.js`
- Triggers: `node index.js` in `bet-placer/`
- Responsibilities: Launches 3 Chromium windows (one per venue), initializes adapters, starts WS server on port 3402, runs interactive CLI

**DK Proxy:**
- Location: `dk-proxy.js` (project root)
- Triggers: `node dk-proxy.js` in project root
- Responsibilities: HTTP server on port 3403, forwards DK league API requests

## Error Handling

**Strategy:** Fail-safe — any error in bet placement aborts the trade without placing leg 2 (avoids one-sided exposure from a different cause). Errors are logged but do not crash the polling loop.

**Patterns:**
- Odds fetch failures: logged as warnings; empty `Vec` returned; book simply doesn't update that venue
- Leg 1 failure: `None` returned from `try_execute`; leg 2 never placed
- Post-fill sigma failure: `None` returned; leg 1 already placed → one-sided exposure deliberately accepted over guaranteed loss
- Circuit breaker trip: breaks out of candidate loop for the cycle; logs warning
- WS connection failure on startup: engine falls back to paper mode automatically

## Cross-Cutting Concerns

**Logging:** `tracing` crate with `INFO` default level; structured fields on some log calls; key prefixes: `[ARB]`, `[LIVE]`, `[PAPER]`, `[BALANCE]`, `[CIRCUIT BREAKER]`, `[RATE LIMIT]`, `[ODDS DRIFT]`, `[HIGH%]`, `[QUOTE]`, `[SKIP]`, `[TIME]`, `[HEALTH]`

**Validation:** Sigma check (`< 1.0`) is the primary correctness gate; applied at detection, post-quote, and post-fill. Selection complementarity validated in `arb_detector`. Spread line signs validated before pairing.

**Authentication:** Browser-session-based (no API keys for soft books). DK: browser cookies via `credentials: "include"`. FD: `context.request.post()` carries session. PIN: `x-session` token extracted from browser `localStorage`, refreshed every 5 minutes.

---

*Architecture analysis: 2026-04-09*
