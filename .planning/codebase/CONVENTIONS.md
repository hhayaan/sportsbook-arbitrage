# Coding Conventions

**Analysis Date:** 2026-04-09

## Languages and Their Conventions

This codebase has two distinct language contexts with separate conventions:
- **Rust** — engine (`engine/src/`)
- **JavaScript (CJS)** — bet placer (`bet-placer/`)

---

## Rust Conventions

### Naming Patterns

**Files:**
- `snake_case` module names matching the struct/logic they contain: `arb_detector.rs`, `odds_fetcher.rs`, `stake_optimizer.rs`, `circuit_breaker.rs`

**Structs and Enums:**
- `PascalCase`: `ArbCandidate`, `MarketIdentityKey`, `VenueSelectionKey`, `CircuitBreaker`, `RateLimiter`

**Functions:**
- `snake_case`: `detect_arbs`, `fetch_all`, `compute_stakes`, `verify_both_legs`, `are_selections_complementary`

**Constants:**
- `SCREAMING_SNAKE_CASE`: `THREE_WAY_SPORTS`, `FILL_TIMEOUT_MS`, `MIN_CAPITAL`, `MIN_PROFIT`, `BETTABLE_VENUES`, `MAX_DATA_AGE_MS`

**Fields:**
- `snake_case` with descriptive prefixes for paired data: `leg1_venue`, `leg1_odds`, `leg2_venue`, `leg2_odds`

**Variables:**
- Short `snake_case` locals, meaningful names at scope boundaries: `now_ms`, `age_a`, `sigma`, `arb_pct`, `trade_key`

### Module Organization

Modules are declared flat in `engine/src/main.rs` with `mod name;` — no nested module trees. Each file is a single-responsibility module:

```rust
// engine/src/main.rs
mod models;
mod config;
mod db;
mod odds_fetcher;
mod arb_detector;
mod stake_optimizer;
mod paper_executor;
mod live_executor;
// ...
```

Internal helpers are private functions in the same file. No barrel re-exports.

### Struct Derivation Pattern

All data-transfer structs derive the same set:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbCandidate { ... }
```

Internal-only structs skip `Serialize`/`Deserialize`:
```rust
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct MarketIdentityKey { ... }
```

### Error Handling

**Pattern: `Result<T, String>` for recoverable errors, `.ok()` to silently discard DB errors.**

Public functions that can fail return `Result<T, String>` with human-readable messages:
```rust
pub fn verify_leg(book: &Book, candidate: &ArbCandidate, leg_num: u8, max_drift: f64) -> Result<f64, String> {
    // ...
    return Err(format!("{}: market no longer in book", leg_label));
}
```

Functions that return optional data use `Option<T>`:
```rust
pub fn compute_rescue(...) -> Option<RescuePlan>
pub fn compute_stakes(candidate: &ArbCandidate, capital: f64) -> Option<StakePlan>
```

Database operations use `.ok()` to silently discard errors — intentional, non-critical:
```rust
conn.execute("INSERT ...", params![...]).ok();
```

Critical initialization uses `.expect()` with descriptive message:
```rust
let conn = Connection::open(path).expect("failed to open sqlite db");
conn.execute_batch("...").expect("migration failed");
```

Mutex locks always use `.unwrap()` — panicking on poisoned lock is intentional:
```rust
let conn = self.conn.lock().unwrap();
```

**In async execution paths (live_executor.rs), errors propagate via `Result<_, String>` and are logged then discarded:**
```rust
Err(e) => {
    tracing::error!("LEG 1 ERROR: {}.", e);
    return None;
}
```

### Logging

Framework: `tracing` crate with structured macros.

**Log level conventions:**
- `tracing::info!` — normal operation events, arb detection, fills, balance updates
- `tracing::debug!` — near-miss arbs, time filter skips, diff cleanup
- `tracing::warn!` — data quality issues, circuit breaker, rate limit hits, high-% arb warning
- `tracing::error!` — bet placement failures, leg failures with exposure warnings

**Log tag prefix pattern** — bracketed uppercase tags for easy grep:
```rust
tracing::info!("[ARB]   {:>5.2}% | {:<5} {:<10} | ...", ...)
tracing::warn!("[CIRCUIT BREAKER] Trading paused: {}", reason)
tracing::warn!("[RATE LIMIT] {} at {}/{} bets/hour", ...)
tracing::info!("[QUOTE] Adjusted stakes: ...")
tracing::info!("[BALANCE] {:<10} = ${:.2} (live)", venue, balance)
tracing::warn!("[HIGH%] {:.1}% arb — possible mismatch: ...")
tracing::info!("[SCALED] Capital ${:.2} → ${:.2} ...")
tracing::warn!("[SPREAD NULL LINE] ...")
tracing::warn!("[HEALTH] {} — no data for {}s", ...)
```

**Field syntax** used in place_leg for structured logging:
```rust
tracing::info!(venue = %venue, stake = stake, selection = %selection, "Sending PLACE_BET via WS");
```

### Comments

**Doc comments (`///`) on public functions and non-obvious logic:**
```rust
/// Compute a rescue hedge when leg 2 fails after leg 1 fills.
/// rescue_stake = leg1_stake * (leg1_odds - 1) / (rescue_odds - 1)
pub fn compute_rescue(...) -> Option<RescuePlan> {
```

**Inline comments explain WHY, not what:**
```rust
// Always false: DK hardcodes is_live=false, PIN uses true for same live game.
// Using actual value breaks cross-venue pairing on live games.
is_live: false,
```

**Section dividers using `──` dashes** inside `models.rs` and other multi-section files:
```rust
// ── Canonical market (normalized from any venue) ────────────────────────────
// ── In-memory order book ────────────────────────────────────────────────────
// ── Arb candidate ───────────────────────────────────────────────────────────
```

### Function Design

- Pure functions with explicit parameters (no global state)
- Safety/filter logic returns early with `continue` or `return None`
- Arithmetic helpers are private single-liners: `fn round_cents(v: f64) -> f64`
- No trait implementations beyond standard derives except `Default` on `Settings`

### Type Aliases

Used for complex async types to keep signatures readable:
```rust
// engine/src/live_executor.rs
pub type WsWriter = Arc<Mutex<SplitSink<...>>>;
pub type WsReader = Arc<Mutex<SplitStream<...>>>;
```

### Shared State

All shared state flows through `Arc<T>`. Mutable shared state uses `Arc<Mutex<T>>` (Db) or `tokio::sync::Mutex` for async contexts. The `AppState` struct in `api.rs` holds `Arc<Db>` and `Arc<WsTx>`.

---

## JavaScript Conventions

### Naming Patterns

**Files:**
- `kebab-case`: `browser-manager.js`, `arb-placer.js`
- `camelCase` for sportsbook adapters: `draftkings.js`, `fanduel.js`, `pinnacle.js`

**Classes:**
- `PascalCase` with `Adapter` suffix: `DraftKingsAdapter`, `FanDuelAdapter`, `PinnacleAdapter`, `BrowserManager`

**Functions and methods:**
- `camelCase`: `executeBet`, `getBalance`, `placeBet`, `calculateBets`, `getSelectionInfo`

**Private methods** — underscore prefix convention:
```javascript
_getPage()       // getter for browser page
_getContext()    // getter for browser context
_apiRequest()    // internal HTTP helper
_getTokens()     // token extraction
_cachedTokens   // cached state field
```

**Constants:**
- `SCREAMING_SNAKE_CASE` at module scope: `HOSTS`, `WAGER_HEADERS`, `X_APPLICATION`, `WS_PORT`, `KEEPALIVE_INTERVAL_MS`, `COOKIES_DIR`

### Module System

CommonJS (`require`/`module.exports`). No ES modules.

```javascript
// Export single class
module.exports = { DraftKingsAdapter, HOSTS };

// Import
const { DraftKingsAdapter } = require("./src/sportsbooks/draftkings");
```

### Error Handling

**Pattern: `throw new Error(message)` in adapters, `try/catch` at the boundary in `index.js`.**

Adapter methods throw on API failure:
```javascript
if (resp.status !== 200) {
  throw new Error(`calculateBets failed (${resp.status}): ${resp.body}`);
}
```

`index.js` handlers catch and send error fill_report:
```javascript
try {
  const result = await adapter.executeBet(venue_market_id, stake);
  // ...
} catch (err) {
  console.error(`[BET] ${attempt_id} FAILED | ${venue} | ${err.message}`);
  ws.send(JSON.stringify({ type: "fill_report", status: "rejected", error_message: err.message }));
}
```

Non-critical operations (balance update after fill) silently swallow errors:
```javascript
try {
  const bal = await adapter.getBalance();
  // ...
} catch (_) {
  // Non-critical — balance update is best-effort
}
```

### Logging

Plain `console.log`/`console.error` — no logging framework.

**Bracketed prefix tag pattern** (mirrors Rust side):
```javascript
console.log(`[DK] Fetching selection info: ${selectionId}`);
console.log(`[BET] ${attempt_id} FILLED | ${venue} | $${stake} @ ${acceptedOdds.toFixed(3)}`);
console.error(`[QUOTE] ${venue} failed: ${err.message}`);
console.log(`[keepalive] ${bookId} refreshed`);
console.log(`[WS] Engine connected`);
```

### JSDoc Comments

All public class methods have JSDoc with `@param` and `@returns`:
```javascript
/**
 * Step 2: Calculate bets — validates the selection and returns a
 * server-signed token required for placement.
 * @param {string[]} selectionIds
 * @returns {{ data: object, calculationId: string }}
 */
async calculateBets(selectionIds) {
```

### Async/Await

All async operations use `async/await` — no raw Promise chains. `Promise`-based polling loops used for confirmation:
```javascript
while (Date.now() - start < maxWaitMs) {
  const resp = await this._apiRequest(url);
  // ...
  await new Promise((r) => setTimeout(r, pollIntervalMs));
}
```

---

## Cross-Cutting Patterns

### Section Dividers

Both sides use ASCII art comment dividers for major logical sections:
- Rust: `// ── Section Name ────────────────────────────────────────────`
- JS: `// ── Section Name ──` or `// ═══... Banner ...═══`

### Monetary Values

Always `f64` in Rust, always JavaScript `number`. Dollar display uses `.toFixed(2)` in JS, `${:.2}` format in Rust. Rounding to cents: `(v * 100.0).round() / 100.0` in Rust (`round_cents`).

### Venue Identifiers

Lowercase string literals throughout: `"draftkings"`, `"fanduel"`, `"pinnacle"`. Never vary capitalization.

### Market Type String Literals

Lowercase with underscores: `"moneyline"`, `"spread"`, `"total"`, `"team_total"`, `"alt_spread"`, `"alt_total"`, `"player_prop"`, `"full_game"`, `"first_half"`, `"regulation"`.

---

*Convention analysis: 2026-04-09*
