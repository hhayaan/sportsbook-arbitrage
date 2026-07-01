# Architecture

This project is a local Ontario sportsbook arbitrage system. Rust owns the engine. Node owns browser-backed sportsbook adapters. The runtime scans odds, normalizes markets, detects two-leg arbitrage, and can either simulate trades or place live bets through logged-in browser sessions.

## Runtime Topology

```text
DraftKings public odds  -> dk-proxy.js :3403 -> Rust engine
FanDuel public odds     ---------------------> Rust engine
Pinnacle logged-in odds -> bet-placer :3404 -> Rust engine

Rust engine :3401
  - odds polling
  - normalization
  - arb detection
  - stake sizing
  - live execution orchestration
  - recovery and telemetry
  - SQLite persistence
  - REST/dashboard WebSocket

Rust engine -> bet-placer WebSocket :3402
  - balances
  - soft validation
  - Pinnacle quote
  - bet placement
  - bet status
  - cashout

bet-placer
  - DraftKings browser adapter
  - FanDuel browser adapter
  - Pinnacle browser/API adapter
```

The bet placer currently launches Patchright Chromium with the `chrome` channel. It uses Google Chrome windows with isolated persistent profiles under the root `data\profiles\<bookId>` directory.

Default ports:

| Port | Process | Purpose |
| ---: | --- | --- |
| 3401 | Rust engine | REST API and dashboard WebSocket |
| 3402 | Node bet placer | Private Rust-to-adapter WebSocket |
| 3403 | `dk-proxy.js` | DraftKings public odds proxy |
| 3404 | Node bet placer | Authenticated Pinnacle odds HTTP bridge |

The engine binds the REST server to `0.0.0.0:3401` with permissive CORS and no auth. Treat it as trusted-local only.

## Source Of Truth

Active runtime configuration comes from:

- source constants in `engine/src/config.rs`;
- SQLite settings in `engine/data/trades.db` when the engine is run from `engine/`;
- startup environment overrides;
- adapter environment variables for Node ports and write timeouts.

`engine/config.toml` is not loaded by the main runtime. Do not use it as the source of operational truth.

## Main Rust Modules

| File | Runtime role |
| --- | --- |
| `engine/src/main.rs` | Startup, settings overrides, poll loop, gates, live/paper branch, REST routes |
| `engine/src/config.rs` | Sports, league IDs, venue constants, poll interval, freshness limit |
| `engine/src/odds_fetcher.rs` | DK/FD/PIN odds fetching and canonical market parsing |
| `engine/src/team_names.rs` | Team/selection normalization and canonical event IDs |
| `engine/src/models.rs` | Runtime models, settings, book, REST/dashboard payloads |
| `engine/src/arb_detector.rs` | Two-leg complementary arb detection |
| `engine/src/odds_verifier.rs` | In-memory drift verification before execution |
| `engine/src/stake_optimizer.rs` | Balance-aware staking and soft-book cap logic |
| `engine/src/execution_math.rs` | Authenticated quote validation and actual-fill hedge math |
| `engine/src/live_executor.rs` | Live protocol, pre-fire checks, placement, recovery, cashout, naked-leg monitor |
| `engine/src/status_classifier.rs` | Raw DK/cashout status classification |
| `engine/src/attempt_telemetry.rs` | JSONL telemetry writer |
| `engine/src/detector_telemetry.rs` | Optional detector funnel and candidate diagnostics |
| `engine/src/family_dedup.rs` | Persistent family-scope live-fire dedup |
| `engine/src/naked_legs.rs` | Durable unresolved-exposure ledger |
| `engine/src/pin_yank.rs` | Temporary Pinnacle hard-yank blacklist |
| `engine/src/paper_executor.rs` | Local simulated execution |
| `engine/src/db.rs` | SQLite schema, settings, balances, trades, arbs |
| `engine/src/api.rs` | Axum REST and dashboard WebSocket handlers |
| `engine/src/ws.rs` | Dashboard broadcast channel |

Helper modules `state_machine.rs`, `treasury.rs`, `policy.rs`, `trades.rs`, and `rescue_hedger.rs` compile but are not the primary execution path.

## Odds Sources

DraftKings:

- Rust calls `http://127.0.0.1:{DK_PROXY_PORT}/dk/{leagueId}`, default port `3403`.
- The proxy uses Node `fetch` against DraftKings public Ontario endpoints.
- Rust falls back to direct DraftKings public fetch if the proxy path fails.
- When `EXPANDED_MARKET_SCAN` is enabled, Rust also calls `http://127.0.0.1:{DK_PROXY_PORT}/dk-event/{eventId}/{subcategoryId}` for HAR-backed per-event subcategory markets such as player props and alternate line menus.

FanDuel:

- Rust calls Ontario content-managed-page endpoints.
- Soccer uses the FanDuel event-type path once, then maps supported competition names into the configured league namespaces: `MLS`, `EPL`, `LA_LIGA`, `SERIE_A`, `BUNDESLIGA`, `LIGUE_1`, and `UCL`.
- Unsupported FanDuel soccer competitions are skipped instead of being compared under generic `SOCCER`.
- FanDuel scan data is public. Authenticated FD checks happen later through `validate_soft`.
- When `EXPANDED_MARKET_SCAN` is enabled, Rust fetches FanDuel event-page payloads for discovered event IDs and parses normal two-way line markets plus over/under player props.

Pinnacle:

- Rust defaults to `PIN_ODDS_SOURCE=auth`.
- Auth mode calls `GET /pinnacle/snapshot/{leagueId}` on `PIN_AUTH_ODDS_BASE_URL`, default `http://127.0.0.1:3404`.
- The bet placer uses the single logged-in Pinnacle browser session to fetch authenticated Arcadia `matchups`, `markets/straight`, and, when enabled, related matchups plus related straight markets for specials.
- Guest mode exists only when `PIN_ODDS_SOURCE=guest` or `PIN_ODDS_SOURCE=public`.

Expanded scanner fanout is controlled by `EXPANDED_MARKET_SCAN`, default true, and `EXPANDED_MARKET_MAX_EVENTS_PER_SPORT`, default `8`.

Every odds fetch uses `ODDS_FETCH_TIMEOUT_MS`, default `2500`. Slow league requests should not hold the whole poll cycle for the full HTTP client timeout.

## Canonical Market Model

Each source price becomes a `CanonicalMarket` with:

- event ID;
- sport and league;
- start time;
- market type;
- surface key;
- normalized selection;
- line;
- period;
- player name and prop type when applicable;
- live flag;
- decimal and American odds;
- venue;
- venue market ID;
- venue event ID;
- received timestamp.

Canonical event IDs are built from normalized team names and start time:

```text
NBA:2026-03-15T19:BOS-vs-GSW
```

The team pair is sorted after normalization. The suffix is a stable cross-book identity key, not an away/home guarantee.

Market identity includes:

```text
event_id + market_type + line_key + period + player_key
```

The actual identity also includes `surface_key`, so game totals, team totals, and player props cannot collide when they share an over/under line shape. It intentionally ignores mismatched live flags because sources can disagree about live state for the same game.

## Detection Scope

Current detection is limited to two-leg arbs:

- moneyline;
- spread;
- total;
- team total;
- player prop over/under, when the scanner can build an exact event, player, prop type, period, and line identity.

Current restrictions:

- live execution candidates must include Pinnacle;
- soccer-style three-way moneyline markets are skipped;
- draw selections are rejected;
- partial-period moneylines are skipped;
- totals require over/under complements;
- player props require over/under complements and fail-closed player identity;
- spreads require different selections and opposite line signs.

Arb math:

```text
sigma = 1 / leg1_decimal_odds + 1 / leg2_decimal_odds
arb_pct = 1 - sigma
roi = 1 / sigma - 1
```

Only `sigma < 1.0` is executable.

Optional detector diagnostics are available without changing terminal output. Set `DETECTOR_TELEMETRY=1` or `DETECTOR_DIAGNOSTICS=1` to write JSONL events for:

- source market counts by venue, sport, market type, and period;
- venue-pair funnel summaries, including shared identity keys, rejected stale pairs, spread rejects, no-Pinnacle rejects, sigma failures, near misses, and candidates;
- compact Pinnacle-involved candidate/near-miss snapshots with venue market IDs and venue event IDs;
- candidate, min-edge, and family-dedup gate summaries.

## Live Execution Model

Live mode uses the WebSocket at `WS_URL`, default `ws://127.0.0.1:3402`.

`LIVE_ALLOWED_PAIRS` can restrict live placement pairs without changing scanning. For example, `LIVE_ALLOWED_PAIRS=fanduel/pinnacle` still scans and prints DK/PIN opportunities, but only FD/PIN candidates can reach live execution.

`LIVE_ALLOWED_MARKET_TYPES` restricts which detected market types can reach live execution. The default is `moneyline,spread,total,team_total`, so player props are scan/paper only by default even when detected.

For Pinnacle-backed arbs, the normal path is:

1. Quote Pinnacle through the logged-in adapter.
2. Validate quote price, min stake, and max stake.
3. Size the soft-book leg with `MAX_STAKE` as the soft-book cap.
4. Validate DK/FD soft price through authenticated adapter calls.
5. Requote Pinnacle immediately before sending the soft leg.
6. Place soft leg.
7. Recompute Pinnacle stake from actual soft fill.
8. Place Pinnacle hedge.
9. Persist live trade only after both legs fill.

`MAX_STAKE` is not a Pinnacle hedge cap for Pinnacle-backed arbs. Pinnacle hedge size is bounded by authenticated quote max, Pinnacle balance, and profitability checks.

Pinnacle fill reports are expected to be automated. The adapter sends accepted IDs as strings, and Rust also accepts numeric IDs. The engine does not use an operator-manual "unknown fill" placement state.

## Recovery Model

Recovery starts only after leg 1 fills and the normal leg-2 path cannot safely finish.

Recovery can:

- fetch soft-book status;
- capture DK/FD cashout offers;
- quote original and adjacent Pinnacle rescue markets;
- compare rescue loss against cashout loss;
- place a Pinnacle rescue hedge;
- cash out DK/FD when supported and available;
- persist unresolved exposure to `data/naked-legs.json`;
- reload persisted exposures and retry status/cashout/original-line relist recovery on later poll cycles.

Cashout attempts and results are visible in JSONL telemetry. Terminal output does not currently print every cashout command/result.

## Family Dedup

The main loop uses persistent family-scope dedup before actual live WebSocket execution. Paper/scan runs do not write persistent family locks.

The default lock file is:

```text
data/family-dedup.json
```

The family key is:

```text
sport + event_id + soft_venue + market_type + period + surface_key + player_name + prop_type
```

The key is order-independent for Pinnacle/soft-book pairs and intentionally excludes line and side. Surface, player, and prop type are included so unrelated props and team totals do not suppress each other. Successful two-leg fills record the family, start a short success cooldown, and allow limited re-entry after the cooldown. Clear soft leg-1 limit/exposure or unavailable-market failures hard-lock the family for the TTL.

Default TTL is 6 hours. Successful fills default to a 30-second cooldown and 3 allowed repeats after the first fill. `FAMILY_DEDUP_FILE`, `FAMILY_DEDUP_TTL_MS`, `FAMILY_DEDUP_TTL_SECS`, `FAMILY_DEDUP_SUCCESS_COOLDOWN_MS`, `FAMILY_DEDUP_SUCCESS_COOLDOWN_SECS`, and `FAMILY_DEDUP_SUCCESS_MAX_REPEATS` can override the defaults. `DEDUP_COOLDOWN_MS` and `DEDUP_COOLDOWN_SECS` are accepted as compatibility aliases for the TTL.

True Hunter-style ladder buckets, ladder allowlists, and family edge-improvement gates are not merged.

## Persistence

SQLite lives at `data/trades.db` relative to the engine working directory. When run from `engine/`, this is:

```text
C:\dev\sportsbook-arbitrage\engine\data\trades.db
```

SQLite tables:

- `book_balances`;
- `paper_trades`;
- `arb_opportunities`;
- `settings`.

Runtime files:

- `data/events.jsonl`;
- `data/bets/<attempt_id>.jsonl`;
- `data/naked-legs.json`;
- `data/family-dedup.json`.

Detector diagnostics, when enabled, are written into the same JSONL telemetry stream.

`ATTEMPT_TELEMETRY_DIR`, `NAKED_LEGS_FILE`, and `FAMILY_DEDUP_FILE` can override the telemetry, naked-leg, and family-dedup locations.

## Current Risk Items

These are current engineering items:

- Expanded DK/FD/PIN player-prop markets are scan/paper capable, but live prop placement is not complete.
- `player_prop` should remain outside `LIVE_ALLOWED_MARKET_TYPES` until Pinnacle special quote/place and prop-aware recovery are implemented.
- Expanded scanner fanout increases poll-cycle work; tune `EXPANDED_MARKET_MAX_EVENTS_PER_SPORT` during live telemetry review.
- Unknown Pinnacle period numbers are skipped instead of mapped to `full_game`; non-full-game Pinnacle moneylines are also skipped during parsing.
- True ladder buckets and edge-improvement gates are not merged; current family dedup is intentionally broad.
- FD authenticated scan overlay is not merged; use detector telemetry and live validation logs before deciding whether public FD scan prices are a recurring problem.
- Settlement ingest is not implemented.
- Cashout retry scheduling is minimal.
- Live WebSocket reconnect after startup is not implemented.
- If live startup cannot connect to the bet placer, the engine falls back to paper execution.
- FanDuel balance scraping is fragile.
- The bet-placer keepalive reload can interfere with active browser state.
- The REST API has no auth.
