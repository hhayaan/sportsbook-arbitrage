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

Soft book FIRST, Pinnacle second. Pre-flight quote still runs before any bets.

1. **Pre-flight quote** — if either leg is Pinnacle, quote it first to get min/max stakes and current price. Adjust stakes if needed.
2. **Quote sigma revalidation** — recalculate sigma with quoted price. If >= 1.0, abort (arb closed). No bets placed.
3. **Place leg 1 (soft book)** — FD or DK goes first. If rejected = zero exposure.
4. **Post-fill sigma revalidation** — recalculate sigma with actual fill odds. If >= 1.0, abort leg 2 (one-sided exposure better than guaranteed loss).
5. **Place leg 2 (Pinnacle)** — Pinnacle almost never rejects.

When both legs are soft books (FD+DK): blocked by `require_pinnacle_leg=true`. DK-FD arbs lost money.

## Balance Management

1. On engine startup (live mode): engine requests real balances from the bet placer
2. After each bet fill: the bet placer sends `balance_update` with the real sportsbook balance
3. Every 60 cycles (~5 min): periodic balance refresh picks up winnings from settled bets
4. Balance-aware stake scaling: if venue balance is too low, stakes scale down instead of rejecting

## Deduplication

After executing a trade, the match key is placed on a 60-second cooldown. Any arb candidate with the same event + market type + venue pair is skipped during this window to prevent duplicate bets.

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
| `MAX_HOURS_TO_START` | `6` | Max hours before event start (capital lockup prevention) |
| `BALANCE_REFRESH_CYCLES` | `60` | Periodic balance sync interval (~5 min at 5s polling) |
| `WS_URL` | `ws://127.0.0.1:3402` | Bet placer address |

## Sports Covered

NBA, NHL, MLB, NFL, NCAAB, NCAAF, EPL, La Liga, Serie A, Bundesliga, Ligue 1, UCL, MLS, UFC

## Team Name Normalization

200+ team names across all sports are normalized to canonical codes (e.g., "Golden State Warriors", "GS Warriors", "Warriors" all -> `GSW`). Match keys use format: `{SPORT}:{DATE}:{AWAY}-vs-{HOME}` to join the same game across venues.
