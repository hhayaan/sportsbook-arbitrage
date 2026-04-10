# Testing Patterns

**Analysis Date:** 2026-04-09

## Test Framework

**Runner:**
- Rust built-in test harness (`cargo test`)
- No external test crate (no `mockall`, `proptest`, etc.)
- Config: none — uses Cargo's default test runner

**Assertion Library:**
- Rust standard `assert!`, `assert_eq!`, `assert!(x.abs() < 0.001)` for float comparisons

**Run Commands:**
```bash
cd engine && cargo test                   # Run all tests
cd engine && cargo test -- --nocapture    # Show println! output
cd engine && cargo test test_name         # Run a specific test
```

**JavaScript testing:**
- No test framework installed (`package.json` has no test dependencies)
- No test files exist in `bet-placer/`
- Manual testing via CLI commands built into `index.js` (`balance`, `info`, `bet`)

## Test File Organization

**Location:** Co-located — tests live at the bottom of the same `.rs` file as the code under test.

**Naming:**
- Module: `mod tests { ... }`
- Functions: `test_<description>` e.g. `test_loss_breaker_trips`, `test_verify_catches_leg2_drift`

**Structure:**
```
engine/src/
├── circuit_breaker.rs     # contains mod tests { 3 tests }
├── odds_verifier.rs       # contains mod tests { 5 tests }
├── rate_limiter.rs        # contains mod tests { 3 tests }
├── rescue_hedger.rs       # contains mod tests { 3 tests }
└── team_names.rs          # contains mod tests { 10 tests }
```

Modules with NO inline tests: `arb_detector.rs`, `stake_optimizer.rs`, `live_executor.rs`, `main.rs`, `db.rs`, `api.rs`, `policy.rs`, `models.rs`, `config.rs`

## Test Structure

**Suite Organization:**
```rust
#[cfg(test)]
mod tests {
    use super::*;  // always present

    // Optional: helper factories
    fn make_market(venue: &str, selection: &str, odds: f64) -> CanonicalMarket { ... }
    fn make_candidate(odds1: f64, odds2: f64) -> ArbCandidate { ... }

    #[test]
    fn test_specific_behavior() {
        // Arrange
        let mut cb = CircuitBreaker::new(50.0, 1800, 0.5, 10, 300);
        // Act
        cb.record(-30.0, false);
        cb.record(-25.0, false);
        // Assert
        assert!(cb.check().is_some()); // $55 loss > $50 threshold
    }
}
```

**No setup/teardown pattern** — each test constructs fresh state inline.

## Mocking

**Framework:** None — no mock crates used.

**Approach:** Real implementations with test-controlled inputs. The `Book` struct is instantiated directly in tests with `Book::new(60000)`, populated with `book.update(&make_market(...))`, and passed to the function under test.

**What is tested with real objects:**
- `Book` (in-memory order book) — used in odds_verifier and rescue_hedger tests
- `CircuitBreaker` — constructed directly
- `RateLimiter` — constructed directly

**What is NOT mocked or tested:**
- Network calls (`fetch_all`, `fetch_draftkings`, `fetch_fanduel`, `fetch_pinnacle`)
- WebSocket communication (`live_executor::try_execute`)
- SQLite DB (`db.rs`)
- Browser automation (`bet-placer/`)

## Fixtures and Factories

**Pattern:** Private `make_*` helper functions inside `mod tests`, returning fully-constructed model objects.

```rust
// From engine/src/odds_verifier.rs
fn make_market(venue: &str, selection: &str, odds: f64) -> CanonicalMarket {
    CanonicalMarket {
        event_id: "NBA:2026-03-15:LAL-vs-BOS".into(),
        sport: "NBA".into(),
        league: "NBA".into(),
        start_time_utc: "2026-03-15T20:00:00Z".into(),
        market_type: "moneyline".into(),
        selection: selection.into(),
        line: None,
        period: "full_game".into(),
        is_live: false,
        decimal_odds: odds,
        american_odds: Some(100.0),
        venue: venue.into(),
        venue_market_id: format!("{}-ml", venue),
        venue_event_id: format!("{}-ev", venue),
        received_at_ms: 1000,
    }
}

fn make_candidate(odds1: f64, odds2: f64) -> ArbCandidate {
    let sigma = 1.0 / odds1 + 1.0 / odds2;
    ArbCandidate {
        event_id: "NBA:2026-03-15:LAL-vs-BOS".into(),
        // ... all fields ...
        sigma, arb_pct: 1.0 - sigma, roi: 1.0 / sigma - 1.0,
        line: None, is_live: false,
    }
}
```

The same `make_market` pattern is repeated in both `odds_verifier.rs` and `rescue_hedger.rs` — it is copy-pasted, not shared via a test utility module.

**Location:** Inline in each module's `mod tests` block. No shared fixture file.

## Coverage

**Requirements:** None enforced — no coverage tooling configured.

**Covered modules:**
- `circuit_breaker.rs` — core business logic (loss + rejection breaker)
- `rate_limiter.rs` — per-venue sliding window
- `odds_verifier.rs` — drift detection (both legs, pass/fail, missing entry)
- `rescue_hedger.rs` — hedge calculation (best odds selection, cost filtering)
- `team_names.rs` — normalization, canonical ID generation, cross-venue matching

**Not covered:**
- `arb_detector.rs` — the core sigma calculation and safety filters have no tests
- `stake_optimizer.rs` — Kelly/proportional stake math has no tests
- `policy.rs` — gate check logic has no tests
- `live_executor.rs` — bet placement flow has no tests
- `db.rs` — database queries have no tests
- `api.rs` — HTTP handlers have no tests
- All of `bet-placer/` — zero JS tests

## Test Types

**Unit Tests:**
- All existing tests are unit-style: test one module's public function in isolation
- State is constructed inline; no shared test DB or network

**Integration Tests:**
- None — no `engine/tests/` directory exists

**E2E Tests:**
- Not used. Manual verification only via CLI commands in `bet-placer/index.js`

## Common Patterns

**Float assertion (no epsilon helper — done inline):**
```rust
assert!((p.stake - 53.33).abs() < 0.01);
assert!((p.odds - 2.5).abs() < 0.001);
```

**Option/Result pattern testing:**
```rust
assert!(cb.check().is_none());          // no block
assert!(cb.check().is_some());          // blocked
assert!(verify_both_legs(...).is_ok());
assert!(verify_both_legs(...).is_err());
assert!(plan.is_some());
assert!(plan.is_none());
```

**Test comments explaining magic numbers:**
```rust
// $55 loss > $50 threshold
assert!(cb.check().is_some());

// Net: -25, below threshold
assert!(cb.check().is_none());
```

**Cross-venue matching test (team_names.rs):**
```rust
// DK names
let dk = canonical_event_id("NBA", "Warriors", "Celtics", "2026-03-15T19:30:00Z");
// FD names
let fd = canonical_event_id("NBA", "Golden State Warriors", "Boston Celtics", "2026-03-15T19:30:00Z");
assert_eq!(dk, fd);  // confirms normalization works across venues
```

---

*Testing analysis: 2026-04-09*
