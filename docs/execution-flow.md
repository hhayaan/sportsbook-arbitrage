# Execution Flow

## Scan -> Detect -> Execute Pipeline

```
1. SCAN — Fetch odds from DK/FD/PIN concurrently (every 5s)
       │
2. BOOK UPDATE — Diff-based: only update entries where odds changed
       │
3. EXPIRE — Remove entries older than 30s
       │
4. DETECT — For each market identity key:
       │     Group prices by selection
       │     For each cross-venue pair: sigma = 1/odds_a + 1/odds_b
       │     If sigma < 1.0: arb candidate found
       │     Draw selections excluded (3-way soccer moneylines are not valid 2-way arbs)
       │
5. SAFETY CHECKS (per candidate):
       │  ├── Circuit breaker: cumulative loss or rejection rate
       │  ├── Rate limiter: per-venue bets/hour
       │  ├── Odds drift: recheck book, reject if >3% drift
       │  └── Bettable venues: skip if either leg is non-bettable
       │
6. STAKE SIZING — Proportional split to equalize payouts:
       │     leg1_stake = capital * (1/leg1_odds) / sigma
       │     leg2_stake = capital * (1/leg2_odds) / sigma
       │
7. EXECUTE — Send PlaceBetCommand via WebSocket
       ├── LEG 1 (soft book first, FD when applicable)
       │     └── If fails: stop, no leg 2 placed
       └── LEG 2
             └── If fails after leg 1: WARNING one-sided exposure
```

## Sigma Formula

```
sigma = 1/decimal_odds_a + 1/decimal_odds_b
```

| Sigma | Meaning |
|-------|---------|
| 0.95 | 5% guaranteed profit (strong arb) |
| 0.99 | 1% guaranteed profit (thin arb) |
| 1.00 | Break-even |
| 1.03 | 3% house edge (not an arb) |

## Stake Optimization

Goal: split total capital so the payout is identical regardless of which side wins.

```
leg1_stake = capital * (1 / leg1_odds) / sigma
leg2_stake = capital * (1 / leg2_odds) / sigma
guaranteed_payout = capital / sigma
guaranteed_profit = guaranteed_payout - capital
```

Example with sigma=0.97, capital=$2.00:
- Leg 1: Team A @2.10 -> stake $0.98
- Leg 2: Team B @1.95 -> stake $1.06
- Guaranteed payout: $2.06 (profit: $0.06)

## Execution Ordering

Leg order: Pinnacle first -> FanDuel first -> DraftKings first.

- **Pinnacle first** when it's one of the legs — the engine does a pre-flight quote to get `minRiskStake`/`maxRiskStake` from the venue. If the calculated stake is below minimum, it bumps it up and recalculates the other leg. If above maximum, it caps and recalculates. If the adjusted total exceeds `MAX_STAKE`, the arb is skipped entirely. No bets are placed until the quote passes.
- **FanDuel first** when it's a FD+DK arb — FD is a soft book, more likely to reject. If FD rejects, DK is never placed.
- **DraftKings** goes second in all cases.

## Deduplication

After executing a trade, the match key is placed on a 60-second cooldown. Any arb candidate with the same event + market type + venue pair is skipped during this window to prevent duplicate bets.

## Live Balance Sync

1. On engine startup (live mode): engine requests real balances from the bet placer via `get_balances` WS message
2. After each bet fill: the bet placer sends `balance_update` with the real sportsbook balance
3. The engine writes these to SQLite so the REST API reflects actual balances

## Engine Configuration

All settable via environment variables at launch:

| Env Var | Default | Description |
|---------|---------|-------------|
| `LIVE_MODE` | `0` | `1` = real bets, `0` = paper trading |
| `FIXED_STAKE` | `25` | $ per leg (fixed mode) |
| `MAX_STAKE` | `50` | Hard cap per leg |
| `MIN_ARB_PCT` | `0.5` | Minimum profit % to trigger |
| `STAKE_MODE` | `fixed` | `fixed` or `proportional` |
| `PROPORTIONAL_PCT` | `5` | % of balance per trade |
| `ENABLED` | `true` | `false` = scan only |
| `WS_URL` | `ws://127.0.0.1:3402` | Bet placer address |

## Sports Covered

NBA, NHL, MLB, NFL, NCAAB, NCAAF, EPL, La Liga, Serie A, Bundesliga, Ligue 1, UCL, MLS, UFC

## Team Name Normalization

200+ team names across all sports are normalized to canonical codes (e.g., "Golden State Warriors", "GS Warriors", "Warriors" all -> `GSW`). Match keys use format: `{SPORT}:{DATE}:{AWAY}-vs-{HOME}` to join the same game across venues.
