# Market Expansion Plan

This file is the implementation tracker for expanding the terminal-first Rust engine beyond the original main-game markets. It should be read with `docs/market_expansion_report.md` for the strategic market research and `docs/HARs/PARTNER-HANDOFF_har-capture_2026-06-04.md` for the captured request evidence.

Last updated: 2026-07-01.

## Current State

The engine now has two scan layers:

- The original hot path for league-level game markets: moneyline, spread, total, and team total.
- A HAR-backed expanded scan path for per-event and related markets from DraftKings, FanDuel, and Pinnacle.

The expanded scan path is enabled by default with `EXPANDED_MARKET_SCAN=1` and is capped by `EXPANDED_MARKET_MAX_EVENTS_PER_SPORT`, default `8`, so the main polling loop does not fan out across every event on every cycle.

Live execution is still conservative. `LIVE_ALLOWED_MARKET_TYPES` defaults to:

```text
moneyline,spread,total,team_total
```

That means player props can be scanned, detected, printed, and paper-tested, but `player_prop` candidates are skipped in live mode unless the operator explicitly opts in. Do not opt in to live player props until the prop placement work below is complete.

## Implemented On 2026-07-01

### Canonical Identity

`CanonicalMarket` now carries `surface_key`, and the identity/dedup path carries the extra dimensions needed for expanded markets:

- `MarketIdentityKey` includes `surface_key`.
- `PriceEntry` carries `surface_key`, `player_name`, and `prop_type`.
- `ArbCandidate` carries `surface_key`, `player_name`, and `prop_type`.
- Family dedup includes surface, player, and prop type so unrelated props or team totals cannot suppress each other.
- `team_names::normalize_selection` treats `player_prop` as an over/under market.

Default game markets can leave `surface_key` empty. Expanded surfaces set explicit keys, for example:

```text
player:assists:jalen brunson:full_game
team_total:toronto blue jays
```

### DraftKings Expanded Scan

The DK proxy now exposes:

```text
GET /dk-event/{eventId}/{subcategoryId}
```

Rust uses that route to call the HAR-observed DraftKings event-subcategory endpoint:

```text
/sites/CA-ON-SB/api/sportscontent/controldata/event/eventSubcategory/v1/markets
```

The scanner collects event IDs from the league feed, then fetches HAR-backed subcategory IDs seen in the collected MLB, NBA, and NHL captures. It emits:

- Two-sided player prop over/under markets when player, prop type, line, and both sides are structurally available.
- Main and alternate totals/spreads when the event-subcategory payload has a normal two-way line shape.
- Team totals with a team-specific `surface_key`.

It intentionally skips one-sided milestones, bands, exact outcomes, race-to markets, and parlay/combo markets because the current detector only supports two-leg complementary arbs.

### FanDuel Expanded Scan

The FanDuel scanner now fetches per-event `event-page` payloads after the league/page fetch discovers event IDs. It emits:

- Two-sided player prop over/under markets.
- Alternate player prop ladders where the line can be read from handicap or runner text.
- Game totals, spreads, and team totals when the event-page payload has a normal two-way line shape.

FanDuel does not expose a portable player ID in these HARs, so cross-book player matching is fail-closed on normalized full player name, event, prop type, line, and period.

### Pinnacle Expanded Scan

The authenticated Pinnacle bridge now includes related data in league snapshots:

- `related_matchups`
- `related_markets`

This is controlled by:

```text
PIN_SNAPSHOT_RELATED=1
PIN_RELATED_MAX_MATCHUPS=8
```

Guest scan mode also fetches related matchups and related straight markets when explicitly selected with `PIN_ODDS_SOURCE=guest` or `PIN_ODDS_SOURCE=public`.

The Rust scanner parses Pinnacle player-prop specials from the related-market payloads and emits two-sided over/under `player_prop` markets when the special has enough structure. Pinnacle special markets use a guarded venue-market-id grammar:

```text
SP:{specialMatchupId}:{marketKey}:{participantId}
```

The `SP:` prefix is intentional. It prevents the existing straight-bet adapter from accidentally treating a special/player-prop market as a normal straight market.

### Live Gate

The main loop now applies `LIVE_ALLOWED_MARKET_TYPES` before live WebSocket execution. This gate is separate from `LIVE_ALLOWED_PAIRS`.

Examples:

```powershell
$env:LIVE_ALLOWED_MARKET_TYPES = "moneyline,spread,total,team_total"
$env:LIVE_ALLOWED_PAIRS = "fanduel/pinnacle"
```

With the defaults, player props remain scan/paper only. When a live candidate is skipped by this gate, telemetry emits `LIVE_MARKET_TYPE_SKIPPED`.

## HAR Coverage

Current HAR files under `docs/HARs` cover:

| File | Book | Coverage now used by scanner |
| --- | --- | --- |
| `dk_mlb_all-markets_padres-at-phillies_placed-then-cashedout_redacted.har` | DK | MLB per-event categories, game lines, inning menus, player props, team totals where two-way |
| `fd_mlb_all-markets_padres-at-phillies_placed-then-cashedout_redacted.har` | FD | MLB event-page markets, game lines, inning menus, player props, team totals where two-way |
| `pin_mlb_straight-ml-total-period_padres-phillies_redacted.har` | PIN | MLB straight full-game and period markets; confirms `s;1` is first five innings, not first inning |
| `pin_player-prop-special-submit_redacted.har` | PIN | Player-prop special market grammar and related-market structure |
| `dk_nba-nhl_player-props_placed-then-cashedout_redacted.har` | DK | NBA/NHL player-prop event subcategories |
| `fd_nba-nhl_props-and-sog_placed_redacted.har` | FD | NBA/NHL event-page props, SOG-style markets, and alternate prop ladders |

The scanner does not yet have HAR-backed implementations for soccer corners, golf matchups, F1 markets, or UFC goes-distance because those HARs are not in the current folder.

## Strategic Correction: MLB First Inning And NRFI

The original report treated MLB first-inning/NRFI as a likely early surface. The HARs changed that conclusion.

DraftKings and FanDuel expose true first-inning total or NRFI-style markets. Pinnacle's MLB period `s;1` in the collected straight-market HAR is first five innings, with lines such as 3.5 and 4.5. It is not first inning.

Because current live execution requires one Pinnacle leg, true first-inning/NRFI is not a live arb surface right now. It can remain a scan-only DK/FD research surface, but it should not be treated as a Pinnacle-backed live expansion target unless a true Pinnacle first-inning market is captured later.

## Current Market Readiness

| Surface | Scan status | Live placement status | Notes |
| --- | --- | --- | --- |
| Game moneyline | Existing | Enabled by default | Pinnacle leg required for live |
| Game spread | Existing | Enabled by default | FD handicap and PIN points paths fixed on 2026-06-25 |
| Game total | Existing | Enabled by default | PIN alt totals supported through straight quote path |
| Team total | Expanded identity in place | Enabled by default, still needs cautious live probes | Uses team-specific `surface_key` |
| DK player props | Implemented for HAR-backed two-way O/U props | Disabled by default | Per-event subcategory fetch |
| FD player props and SOG-style props | Implemented for HAR-backed two-way O/U props | Disabled by default | Event-page fetch; name-only player identity |
| PIN player-prop specials | Implemented for scan/detect/paper | Disabled by default | `SP:` IDs intentionally block straight placement |
| MLB true first inning / NRFI | DK/FD scan research only | Not viable live with current PIN coverage | PIN HAR showed F5, not first inning |
| Soccer corners | Not implemented | Not implemented | Need DK/FD/PIN HARs |
| Golf matchups | Not implemented | Not implemented | Need HARs plus settlement matrix |
| F1 / motorsport | Not implemented | Not implemented | Need HARs plus DNF/classification rules |
| UFC goes distance | Not implemented | Not implemented | Need HARs plus full rule diff |

## Required Before Live Player Props

Player props are not live-ready. The next implementation phase should add:

1. Pinnacle special quote and placement support, likely through `/bets/special` rather than the existing straight path.
2. WebSocket command typing so Rust can tell Node whether a Pinnacle leg is straight or special.
3. Recovery support for half-filled prop arbs, including special-market rescue behavior.
4. Soft-book validation checks for DK/FD props using the captured `calculateBets` and `implyBets` paths.
5. Fixture-backed adapter tests for at least one DK prop, one FD prop, and one PIN special.
6. A settlement/correction matrix per prop family before broad live use.

Until that work is done, keep `player_prop` out of `LIVE_ALLOWED_MARKET_TYPES`.

## Scan-Only Validation Plan

The immediate next goal is a scan-only run using the collected HAR-backed scanner paths:

```powershell
$env:DETECTOR_TELEMETRY = "1"
$env:EXPANDED_MARKET_SCAN = "1"
$env:EXPANDED_MARKET_MAX_EVENTS_PER_SPORT = "8"
cargo run --manifest-path engine\Cargo.toml
```

Review:

- `engine\data\events.jsonl`
- Terminal `[ARB]` output
- `DETECTOR_MARKET_COUNTS`
- `DETECTOR_FUNNEL_SUMMARY`
- `DETECTOR_PIN_PAIR_SNAPSHOT`
- `DETECTOR_CANDIDATE`
- `MIN_EDGE_SKIPPED`
- `LIVE_MARKET_TYPE_SKIPPED`

The first pass should answer:

- Are DK, FD, and PIN emitting the expected number of prop markets?
- Are player prop identities aligning only when event, player, prop type, period, and line match?
- Are any valid-looking candidates suppressed by family dedup?
- Are any markets incorrectly grouped with game totals or team totals?
- How much poll-cycle latency does the expanded fetch add?

## Economics And Telemetry Requirements

Before any expanded live surface is enabled, scan-only and paper runs should measure:

- Markets fetched by book, sport, market type, period, and surface.
- Shared identity keys by venue pair.
- Complementary pair count.
- Candidate count before and after `MIN_ARB_PCT`.
- Pinnacle quote survival rate.
- Soft validation survival rate.
- Average and max quoted stake.
- Family dedup impact.
- Poll-start to live-decision latency.
- Rejection reasons by market type.

## Open Decisions

| Decision | Current recommendation |
| --- | --- |
| Keep Pinnacle mandatory for expanded live markets? | Yes. The runtime still assumes Pinnacle as hedge truth. |
| First live expansion beyond main game markets? | Full-game spreads/totals first; they are already closest to production. |
| First scan-only expansion using current HARs? | Player-prop O/U across DK, FD, and PIN. |
| First-inning/NRFI as live target? | No, not without a true Pinnacle first-inning capture. |
| Soccer corners next? | Yes, but only after DK/FD/PIN HARs are captured. |
| Allow expanded markets live immediately after parser support? | No. Use scan-only, then paper, then controlled tiny live probes. |
| Add settlement ingest before props? | At minimum, define a correction-window and governing-stat policy before broad live use. |

## Working Checklist

Done:

- [x] Read and inventory collected HARs.
- [x] Add canonical `surface_key` to market identity.
- [x] Carry player/prop identity into candidate and dedup paths.
- [x] Add DK per-event event-subcategory scanner path.
- [x] Add FD per-event event-page scanner path.
- [x] Add PIN related/special scanner path.
- [x] Add live market-type gate with `LIVE_ALLOWED_MARKET_TYPES`.
- [x] Add parser tests for DK event-subcategory props, FD event-page props, and PIN related specials.
- [x] Keep `player_prop` out of default live placement.

Next:

- [ ] Run authenticated scan-only telemetry with `EXPANDED_MARKET_SCAN=1`.
- [ ] Compare market counts and identity overlap across DK/FD/PIN.
- [ ] Investigate any detector near-misses or unexpected family-dedup skips.
- [ ] Add sanitized fixtures for the specific HAR-derived payloads if we want replay tests beyond embedded parser-unit examples.
- [ ] Implement Pinnacle special quote/place support.
- [ ] Implement prop-aware soft validation and recovery.
- [ ] Capture soccer-corners HARs for DK, FD, and Pinnacle if soccer corners remain the next non-prop target.

## Definition Of Done For This Phase

This HAR-backed scan phase is done when:

- [x] DK, FD, and PIN can emit expanded two-way O/U prop markets from the collected request shapes.
- [x] Expanded markets have stable canonical identity and do not collide with main game markets.
- [x] Live mode skips prop candidates by default.
- [x] Rust tests and Node syntax checks pass.
- [ ] One authenticated scan-only session confirms the expanded scanner works against live logged-in sessions.
- [ ] Telemetry shows whether any real cross-book prop arbs or near misses survive normalization.
- [ ] The next live-placement phase has fixture-backed quote/place support for Pinnacle specials and DK/FD prop validation.
