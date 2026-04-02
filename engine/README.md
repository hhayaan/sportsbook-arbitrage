# Arb Engine V3

A production-grade sports arbitrage detection and execution engine written in Rust. Designed as a standalone, modular core that plugs into separate ingestion, executor, and dashboard services via Redis.

## Architecture

```
[Ingestion Service]          [Executor Service]          [Dashboard]
  (TypeScript)                 (Playwright)               (Next.js)
       |                            ^                        ^
       v                            |                        |
  Redis Streams              Redis Pub/Sub              PostgreSQL
  markets:{venue}            commands:{venue}           (read-only)
       |                            |                        |
       +----------> [ ARB ENGINE V3 ] <---------------------+
                     (This project)
                          |
                          v
                      PostgreSQL
                    (audit trail)
```

The engine is the **brain** of the system. It:

1. Consumes normalized market data from Redis Streams
2. Detects arbitrage opportunities across sportsbooks
3. Computes optimal stake allocation
4. Executes trades (paper or live) through a modular executor interface
5. Manages risk through multiple safety layers
6. Persists all trade attempts and diagnostics to PostgreSQL

## Quick Start

### Prerequisites

- Rust 1.70+
- Redis 7
- PostgreSQL 16
- Docker (for infrastructure)

### Run Infrastructure

```bash
# Start Redis + PostgreSQL
docker run -d --name redis -p 6379:6379 redis:7-alpine
docker run -d --name postgres -p 5432:5432 \
  -e POSTGRES_USER=arb \
  -e POSTGRES_PASSWORD=arb_dev_password \
  -e POSTGRES_DB=arb_engine_v3 \
  postgres:16-alpine
```

### Build and Run

```bash
cd engine
cargo build --release
cargo run --release
```

### Run Tests

```bash
cargo test
```

58 unit tests across all critical modules.

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `REDIS_URL` | `redis://127.0.0.1:6379` | Redis connection URL |
| `DATABASE_URL` | `postgres://arb:arb_dev_password@localhost:5432/arb_engine_v3` | PostgreSQL connection URL |
| `LOG_DIR` | `logs` | Directory for log files |

## Configuration

All runtime behavior is controlled via `config.toml`. The engine loads this file on startup and falls back to safe defaults if missing.

### Strategy

```toml
[strategy]
require_pinnacle_leg = true   # Only trade arbs with Pinnacle on one side
```

**Why Pinnacle?** Pinnacle is the sharpest sportsbook globally. When DraftKings or FanDuel disagree with Pinnacle's line, the soft book is genuinely mispriced. DK-vs-FD arbs without Pinnacle are more likely to be stale data artifacts or will get rejected/limited. When enabled, skipped DK-FD arbs are logged with sigma/arb% for opportunity cost tracking.

### Policy Gates

```toml
[policy]
min_identity_confidence = 0.95     # Market matching confidence threshold
max_data_age_ms = 60000            # Max quote age (60 seconds)
min_seconds_to_start = 60          # Pre-game buffer (skipped for live)
min_stake = 5.0                    # Minimum per-leg stake ($)
min_profit = 0.50                  # Minimum guaranteed profit ($)
max_rescue_loss = 25.0             # Max acceptable rescue hedge loss ($)
max_event_exposure = 500.0         # Max total $ deployed on one event
max_stake_pct = 0.20               # Max single leg as % of venue balance
dedup_cooldown_secs = 60           # Pre-game dedup cooldown
live_dedup_cooldown_secs = 15      # Live/in-play dedup cooldown
max_odds_drift = 0.03              # Max acceptable odds drift (3%)
source_health_threshold_ms = 30000 # Venue considered unhealthy after 30s silence
```

### Circuit Breaker

```toml
[circuit_breaker]
loss_threshold = 50.0       # Trip if cumulative loss > $50 in window
loss_window_secs = 1800     # 30-minute rolling window
rejection_threshold = 0.50  # Trip if >50% of recent attempts rejected
rejection_window = 10       # Look at last 10 attempts
cooldown_secs = 300         # 5-minute cooldown after trip
```

### Venue Configuration

```toml
[venues.draftkings]
mode = "paper"              # "paper" | "live" | "disabled"
initial_balance = 1000.00
max_stake = 500.00
min_stake = 1.00
max_bets_per_hour = 3

[venues.pinnacle]
mode = "paper"
initial_balance = 2000.00
max_stake = 1000.00
min_stake = 1.00
max_bets_per_hour = 5
```

### Paper Trading Simulation

```toml
[paper_trading]
enabled = true
random_seed = 42          # Deterministic (reproducible) simulation
fill_rate = 0.85          # 85% of bets fill successfully
partial_fill_rate = 0.10  # 10% partial fills
odds_drift_rate = 0.15    # 15% chance of odds moving
rejection_rate = 0.05     # 5% outright rejection
min_latency_ms = 500
max_latency_ms = 2000
```

## Module Reference

### Core Detection

| Module | Lines | Description |
|--------|-------|-------------|
| `arb_detector.rs` | 450+ | Sigma-based arbitrage detection with Pinnacle filter and near-miss tracking |
| `identity.rs` | 281 | Market identity matching across venues (moneyline, spread, total, team_total) |
| `book.rs` | 151 | In-memory order book: identity key -> (venue+selection -> price entry) |
| `canonical.rs` | 40 | `CanonicalMarket` struct - the universal schema for all market data |

### Execution

| Module | Lines | Description |
|--------|-------|-------------|
| `stake_optimizer.rs` | 283 | Unconstrained/constrained stake calculation with $5 humanized rounding |
| `paper_executor.rs` | 137 | Simulated bet placement with configurable latency, drift, and rejection |
| `state_machine.rs` | 184 | 18-state trade lifecycle FSM (CANDIDATE_DETECTED through RECONCILED) |
| `rescue_hedger.rs` | 191 | Automatic hedge computation when leg 2 fails after leg 1 fills |

### Risk Management

| Module | Lines | Description |
|--------|-------|-------------|
| `policy.rs` | 133 | 10 policy gates - ALL must pass before execution |
| `circuit_breaker.rs` | 218 | Loss breaker + rejection rate breaker with cooldown |
| `rate_limiter.rs` | 129 | Per-venue sliding 1-hour bet rate limits |
| `odds_verifier.rs` | 216 | Pre-execution odds drift verification (leg 1 and leg 2) |
| `treasury.rs` | 171 | 3-state capital tracking (free/reserved/in_flight) per venue |

### Diagnostics

| Module | Lines | Description |
|--------|-------|-------------|
| `metrics.rs` | 417 | Funnel metrics, sigma histograms, dimensional tracking, gate rejections |
| `opportunity_tracker.rs` | 213 | Arb window half-life analysis (P25/P50/P75 latency) |
| `near_miss.rs` | 263 | Ring buffer tracking near-misses that later cross into arb territory |

### Persistence

| Module | Lines | Description |
|--------|-------|-------------|
| `db.rs` | 492 | PostgreSQL migrations, trade persistence, balance snapshots, diagnostics |
| `trade_ledger.rs` | 167 | Append-only JSONL trade records (survives DB outages) |
| `trades.rs` | 174 | Structured trade event logging (ARB_DETECTED, LEG_FILL, RESCUE_*, etc.) |

### Infrastructure

| Module | Lines | Description |
|--------|-------|-------------|
| `main.rs` | 1080 | Event loop, orchestration, graceful shutdown |
| `config.rs` | 235 | TOML config loading with serde defaults |
| `redis_bridge.rs` | 95 | Redis Streams read/write (markets, fills, commands) |
| `logging.rs` | 59 | Tracing setup: stdout + core.log + trades.log with daily rotation |

## How Arbitrage Detection Works

### The Math

For any two-outcome market (moneyline, over/under, spread), arbitrage exists when:

```
sigma = 1/odds_leg1 + 1/odds_leg2 < 1.0
```

If sigma < 1.0, you can split capital across both sides and guarantee profit regardless of outcome:

```
guaranteed_profit = capital / sigma - capital
```

### Detection Pipeline

```
Markets ingested from Redis
    |
    v
Book updated (identity key -> venue -> price entry)
    |
    v
Group by identity key (same event + market + line + period + OT rules)
    |
    v
Find complementary selections (LAL vs BOS, over vs under)
    |
    v
Cross-venue comparison: all venue pairs for each complementary pair
    |
    v
Pinnacle filter: skip pairs without Pinnacle leg (if enabled)
    |
    v
Sigma calculation: 1/o1 + 1/o2
    |
    v
sigma < 1.0 -> ArbCandidate (sorted by arb_pct descending)
1.0 <= sigma < 1.02 -> Near-miss (tracked for later-crossing analysis)
sigma >= 1.02 -> No value (recorded in histogram only)
```

### Stake Optimization

1. **Unconstrained**: Deploy capital to lock guaranteed profit
   - `s1 = capital * (1/odds1) / sigma`
   - `s2 = capital * (1/odds2) / sigma`

2. **Constrained**: Respect per-venue min/max stake limits
   - Scale down proportionally if maxes exceeded
   - Reject if profit becomes zero or negative

3. **Humanized**: Round to nearest $5 increments
   - Tests all 4 combinations (floor/ceil per leg)
   - Selects maximum profitable option
   - Falls back to exact stakes if no rounding is profitable

## Trade Lifecycle (State Machine)

```
CANDIDATE_DETECTED
    |
    v
SIZING -> StakePlan computed
    |
    v
CAPITAL_RESERVED -> Treasury locks funds
    |
    v
LEG1_SUBMITTED -> LEG1_AWAITING
    |
    +---> LEG1_FILLED -----> LEG2_SUBMITTED -> LEG2_AWAITING
    |         |                                      |
    |         |                +---------------------+
    |         |                |                     |
    |         |         LEG2 FILLED            LEG2 FAILED
    |         |                |                     |
    |         |         EXECUTION_CLOSED       RESCUE_NEEDED
    |         |         (locked profit)              |
    |         |                               RESCUE_COMPUTED
    |     LEG1_PARTIAL                               |
    |     (scale leg2)                        RESCUE_SUBMITTED
    |                                           |         |
    +---> LEG1_REJECTED                   RESCUE_CLOSED  ABORT_AND_ESCALATE
    |     (release all)                   (hedged loss)  (manual intervention)
    |
    +---> LEG1_UNKNOWN
          (block venue, manual resolution required)
```

## Risk Management Layers

The engine has **5 independent safety layers** that protect capital:

### Layer 1: Policy Gates (10 checks)

ALL must pass before any capital is reserved:

| Gate | Check | Default |
|------|-------|---------|
| Identity Confidence | Market match quality >= threshold | 0.95 |
| Data Age | Quote freshness <= max age | 60s |
| Event Time | Pre-game starts in >= buffer (live: skipped) | 60s |
| Min Stake | Both legs >= minimum | $5 |
| Min Profit | Guaranteed profit >= minimum | $0.50 |
| Venue Balance | Free balance >= required stake | per-venue |
| Source Health | Venue sent data within threshold | 30s |
| Rate Limit | Bets/hour within venue limit | 3-5/hr |
| Odds Drift | Current vs detected odds within tolerance | 3% |
| Circuit Breaker | Not tripped | see below |

### Layer 2: Circuit Breaker

Automatically stops all trading when losses accumulate:

- **Loss breaker**: Trips if cumulative loss exceeds $50 in 30 minutes
- **Rejection breaker**: Trips if >50% of last 10 attempts were rejected
- **Cooldown**: 5-minute pause after either breaker trips
- Resets automatically after cooldown

### Layer 3: Rate Limiter

Per-venue sliding 1-hour windows prevent account flagging:

- DraftKings: 3 bets/hour
- FanDuel: 3 bets/hour
- Pinnacle: 5 bets/hour (sharper book, more tolerant)

### Layer 4: Odds Verification

Pre-execution drift checks at two critical points:

1. **Before capital reservation**: Verify both legs haven't moved >3%
2. **Before leg 2 execution**: Re-verify after leg 1 fills (if drifted, go straight to rescue)

### Layer 5: Rescue Hedging

When leg 2 fails after leg 1 fills, the engine automatically:

1. Finds best available odds for the opposite side on ANY venue
2. Computes rescue stake to minimize worst-case loss
3. Validates worst-case loss <= `max_rescue_loss` ($25 default)
4. Executes rescue bet
5. If no feasible rescue exists: escalates to `ABORT_AND_ESCALATE` (manual review)

## Redis Interface

### Input: Market Data

The engine reads from Redis Streams named `markets:{venue}`:

```json
{
  "event_id": "NBA:2026-03-15:LAL-vs-BOS",
  "sport": "NBA",
  "league": "NBA",
  "market_type": "moneyline",
  "selection": "LAL",
  "decimal_odds": 2.50,
  "venue": "draftkings",
  "received_at_ms": 1711000000000,
  "is_live": false,
  "period": "full_game",
  "includes_overtime": true,
  "push_treatment": "void"
}
```

See `canonical.rs` for the full `CanonicalMarket` schema (28 fields).

### Output: Commands

Bet placement commands are published to Redis Pub/Sub `commands:{venue}`:

```json
{
  "action": "place_bet",
  "venue": "draftkings",
  "selection": "LAL",
  "market_type": "moneyline",
  "stake": 50.0,
  "odds": 2.50,
  "venue_market_id": "dk:12345"
}
```

### Output: Fill Reports

Fill results are read from Redis Streams `fills:{venue}`:

```json
{
  "status": "filled",
  "accepted_stake": 50.0,
  "accepted_odds": 2.48
}
```

## Database Schema

Auto-migrated on startup. Key tables:

| Table | Purpose |
|-------|---------|
| `arb_attempts` | Full trade audit trail (UUID, event, both legs, sigma, profit, state timeline) |
| `paper_trading_balance` | Per-venue balance snapshots (free/reserved/in_flight) |
| `venue_reliability` | Fill rates, rejection counts, avg latency per venue |
| `source_health` | Data feed connectivity monitoring |
| `funnel_metrics` | Periodic snapshots of the detection-to-execution funnel |
| `sigma_histograms` | Distribution of sigma values by venue pair, sport, market type |
| `near_miss_records` | Markets within 2% of arb threshold |
| `opportunity_lifespans` | Arb window duration analysis |
| `gate_rejections` | Policy gate failure attribution |

## Logging

Three-tier output:

| Destination | Level | Content |
|-------------|-------|---------|
| `stdout` | INFO+ | Human-readable, colored |
| `logs/core.log` | DEBUG+ | All events, timestamped, daily rotation |
| `logs/trades.log` | Trade events only | ARB_DETECTED, LEG_FILL, EXECUTION_CLOSED, RESCUE_* |
| `logs/trade_ledger/*.jsonl` | All attempts | Append-only JSONL (survives DB outages) |

## Diagnostic Instrumentation

The engine continuously tracks performance metrics:

### Funnel Metrics (12 stages)

```
markets_ingested -> markets_normalized -> cross_venue_groups ->
complementary_pairs -> sigma_qualified -> near_misses ->
policy_passing -> execution_attempted -> leg1_filled ->
trades_completed + rescues_triggered -> rescues_completed + trades_aborted
```

Logged every 30 seconds with atomic counters.

### Sigma Histogram (8 buckets)

Tracks the distribution of all sigma values:
- Deep arb (< 0.98), Quality arb (0.98-0.99), Marginal arb (0.99-1.00)
- Near miss (1.00-1.005, 1.005-1.01, 1.01-1.02)
- No value (1.02-1.05, >= 1.05)

### Dimensional Sigma Tracking

Sigma histograms broken down by:
- Venue pair (DK-FD, DK-PIN, FD-PIN)
- Sport (NBA, NFL, MLB, etc.)
- Market type (moneyline, spread, total)
- Live vs pre-game

### Opportunity Half-Life

Tracks how long arb windows last:
- P25/P50/P75 duration in milliseconds
- Whether opportunities reached execution before closing
- Death reason classification

## Supported Markets

| Market Type | Complementary Logic |
|-------------|-------------------|
| Moneyline | Any two different teams |
| Spread | Two different teams (line symmetry enforced by identity key) |
| Total (Over/Under) | Over vs Under |
| Team Total | Over vs Under (same team scope) |

## Supported Venues

| Venue | Type | Notes |
|-------|------|-------|
| Pinnacle | Sharp | Hedge leg, highest rate limit |
| DraftKings | Soft | Value leg, lower rate limit |
| FanDuel | Soft | Value leg, lower rate limit |

## Development

### Adding a New Venue

1. Add venue config to `config.toml` under `[venues.newvenue]`
2. Start publishing to Redis Stream `markets:newvenue` from your ingestion service
3. The engine auto-discovers new streams on startup

### Adding a New Market Type

1. Add complementarity logic to `are_selections_complementary()` in `arb_detector.rs`
2. Add identity key construction in `identity.rs`
3. Update tests

### Switching to Live Trading

1. Set `mode = "live"` in venue config
2. Implement executor service that subscribes to `commands:{venue}` and publishes to `fills:{venue}`
3. Set `paper_trading.enabled = false`

## License

Private. All rights reserved.
