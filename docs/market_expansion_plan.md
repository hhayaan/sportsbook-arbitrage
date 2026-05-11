# Market Expansion Plan

This file tracks the planned expansion from the current main market engine into additional market surfaces from `docs/market_expansion_report.md`. It is a planning and implementation tracker, similar in purpose to `docs/engine-merge-plan.md`.

The current project is still terminal-first and Rust-led. Expanded markets should be added deliberately, behind scan-only and live-placement gates, with parser fixtures and source evidence before any live betting.

## Current Position

The engine currently scans and executes a narrow set of canonical two-outcome markets:

- Moneyline.
- Spread.
- Total.
- Limited team-total plumbing, but team totals are not a planned live expansion surface yet.

The current live execution path remains Pinnacle-led:

- Candidate detection requires one Pinnacle leg.
- Authenticated Pinnacle odds are the default scan source.
- Pinnacle executable quote is treated as hedge truth.
- The soft book is validated before risk.
- The soft book is placed first, then Pinnacle is used as the hedge.

Important current limitations:

- There is no dedicated `surface_key` yet.
- `market_type=total` is too broad for expanded markets.
- `CanonicalMarket` has `player_name` and `prop_type`, but the detector, verifier, execution path, DB, telemetry, and family dedup are still effectively built around `event_id`, `market_type`, `line`, and `period`.
- FanDuel line placement has a known baseline risk: `executeBet()` currently calls `implyBets(marketId, runnerId)` without passing the runner handicap. This must be fixed before expanding FanDuel line markets.
- There is no raw response fixture harness for expanded parser work.
- There is no `LIVE_ALLOWED_SURFACES` gate yet.

## Guardrails

- Keep engine logic in Rust.
- Keep browser/session/API automation in `bet-placer` as the adapter bridge.
- Keep the operator surface terminal-first with JSONL telemetry.
- Do not port a GUI or frontend for market expansion.
- Do not enable live placement for a new surface until scan-only behavior is proven.
- Keep Pinnacle mandatory for expanded live markets unless we explicitly decide otherwise later.
- Do not add fuzzy parser matching for new markets. Use explicit allowlisted mappings per book and surface.
- Do not treat public market existence as production readiness. Each surface must pass Ontario availability, API addressability, settlement compatibility, and executable economics checks.
- Do not add player props until stat-source and correction-window rules are mapped.

## Current Work

### HAR Collection

Status: in progress.

Purpose:

- Capture real logged-in Ontario request shapes before parser or placement changes.
- Confirm market IDs, selection IDs, line/handicap fields, runner fields, quote responses, max stake fields, and error behavior.
- Provide fixtures for parser tests and adapter tests.

Current requested HAR set:

| File name | Book | Surface | Needed calls | Placement? |
| --- | --- | --- | --- | --- |
| `fd_baseline_spread.har` | FanDuel | Normal spread | `getMarketPrices`, `implyBets` | No submit |
| `fd_baseline_total.har` | FanDuel | Normal total | `getMarketPrices`, `implyBets` | No submit |
| `dk_mlb_1st_inning_total.har` | DraftKings | MLB first-inning total / NRFI-style | `selectioninfo`, `calculateBets` | No submit |
| `fd_mlb_1st_inning_total.har` | FanDuel | MLB first-inning total / NRFI-style | `getMarketPrices`, `implyBets` | No submit |
| `pin_mlb_1st_inning_total.har` | Pinnacle | MLB first-inning total / NRFI-style | Arcadia `matchups`, `markets/straight`, `/bets/straight/quote` | No submit |
| `dk_soccer_corners.har` | DraftKings | Soccer corners | `selectioninfo`, `calculateBets` | No submit |
| `fd_soccer_corners.har` | FanDuel | Soccer corners | `getMarketPrices`, `implyBets` | No submit |
| `pin_soccer_corners.har` | Pinnacle | Soccer corners | Arcadia `matchups`, `markets/straight`, `/bets/straight/quote` | No submit |

Collection notes:

- Use logged-in Ontario sessions.
- Preserve DevTools Network log.
- Reload the event page after preserve-log is enabled.
- Expand the target market.
- Click one selection into the betslip.
- Enter a tiny stake or stop immediately before final submit.
- Trigger quote/validation only.
- Do not place a bet.
- HAR files can contain cookies, auth headers, geolocation tokens, and session data. Keep them local and do not publish them.

## Next Work After HAR Collection

### 1. Sanitize And Inventory HARs

Goal:

- Extract only the fields needed for parser and adapter implementation.
- Remove cookies, auth tokens, account IDs, geolocation payloads, and unrelated traffic.

Deliverables:

- Sanitized request/response fixtures under an agreed fixture directory.
- A short field inventory per book and surface.
- A note for each captured market saying whether the surface appears Ontario-tradable and quote-addressable.

Recommended fixture structure:

```text
engine/fixtures/
  fanduel/
    baseline_spread_getMarketPrices.json
    baseline_spread_implyBets.json
    baseline_total_getMarketPrices.json
    baseline_total_implyBets.json
    mlb_first_inning_total_getMarketPrices.json
    mlb_first_inning_total_implyBets.json
    soccer_corners_getMarketPrices.json
    soccer_corners_implyBets.json
  draftkings/
    mlb_first_inning_total_selectioninfo.json
    mlb_first_inning_total_calculateBets.json
    soccer_corners_selectioninfo.json
    soccer_corners_calculateBets.json
  pinnacle/
    mlb_first_inning_total_matchups.json
    mlb_first_inning_total_markets_straight.json
    mlb_first_inning_total_quote.json
    soccer_corners_matchups.json
    soccer_corners_markets_straight.json
    soccer_corners_quote.json
```

### 2. Fix FanDuel Handicap Propagation

Why this comes first:

- FanDuel spreads, totals, alternate lines, corners, and many line markets can require an exact runner handicap.
- The current placement flow gets the runner and its `handicap`, but the validation call does not pass it into `implyBets`.
- If `implyBets` receives the wrong handicap, FanDuel can reject with errors such as requested handicap unavailable.

Implementation recommendation:

- Pass `runner.handicap` from `executeBet()` into `implyBets(marketId, runnerId, runner.handicap)`.
- Preserve the existing behavior of using the `implyBets` runner returned by FanDuel for final `placeBet`.
- Add adapter tests or fixture-backed checks using the baseline FD spread and total HARs.

Acceptance criteria:

- Normal FD spread validation includes the correct handicap.
- Normal FD total validation includes the correct handicap when present.
- Existing FD moneyline behavior is unchanged.

### 3. Add Canonical Surface Framework

Goal:

- Prevent collisions between markets that currently look like the same `market_type`, `line`, and `period` but are actually different things.

Problem examples:

- Soccer full-game goals total and soccer corners total are both totals.
- Soccer cards total and soccer corners total are both totals, but have different settlement units.
- MLB full-game total and MLB first-inning total are both totals, but use different periods and settlement windows.
- Player strikeouts over/under and game total over/under are both over/under structures, but one is a player stat and one is a game score.

Recommended model:

- Add `surface_key` to `CanonicalMarket`.
- Carry `surface_key` into `MarketIdentityKey`, `PriceEntry`, `ArbCandidate`, detector telemetry, verifier reconstruction, family dedup, and DB/API rows where needed.
- Keep `market_type` as the bet shape: `moneyline`, `spread`, `total`, `team_total`, `player_prop`, `alt_spread`, `alt_total`, or other explicit shape.
- Use `surface_key` as the thing being measured: examples include `game`, `first_inning_runs`, `soccer_corners`, `soccer_cards`, `golf_matchup`, `ufc_goes_distance`, `f1_driver_matchup`, `player_strikeouts`.

Example:

```text
market_type = total
surface_key = soccer_corners
period = full_game
line = 9.5
selection = over
```

Acceptance criteria:

- Existing moneyline/spread/total behavior defaults to `surface_key=game`.
- Existing tests still pass.
- Soccer corners cannot group with soccer goals totals.
- Future player props cannot group with team/game totals.
- Family dedup keys include surface identity.
- Detector telemetry and attempt telemetry include surface identity.

### 4. Add Scan And Live Surface Gates

Goal:

- Allow us to observe expanded markets without risking live placement.

Recommended env vars:

- `SCAN_ALLOWED_SURFACES`: optional scan allowlist. Defaults to current approved surfaces only until expansion begins.
- `LIVE_ALLOWED_SURFACES`: optional live placement allowlist. Defaults to `game` only.

Recommended behavior:

- Scanning can include new surfaces only when explicitly enabled.
- Terminal output should include surface when it is not `game`.
- Live execution should refuse a candidate whose surface is not allowed, even if the venue pair is allowed.
- `LIVE_ALLOWED_PAIRS` should remain independent of surface gating.

Acceptance criteria:

- We can scan MLB first-inning markets without placing them.
- We can scan soccer corners without placing them.
- Existing main-market live mode remains unchanged when no new env vars are set.

### 5. Add Fixture-Backed Parser Tests

Goal:

- Make parser expansion repeatable and auditable.

For each new surface and book, tests should confirm:

- Correct `event_id`.
- Correct `sport` and `league`.
- Correct `market_type`.
- Correct `surface_key`.
- Correct `period`.
- Correct `selection`.
- Correct `line`.
- Correct venue market ID.
- Correct exclusion of unsupported or ambiguous market variants.

Acceptance criteria:

- The first expanded surface has DK, FD, and Pinnacle parser fixture tests.
- Tests prove that expanded surface markets do not collide with existing game markets.

## First Planned Surface: MLB First-Inning Total / NRFI-Style Markets

Status: planned after HAR collection and baseline infrastructure.

Why it is first:

- It is closest to the existing period plus total/binary model.
- It exercises period identity without immediately requiring player/stat-source plumbing.
- It is a good spike for the `surface_key` framework.

Likely surface mapping:

| Concept | Proposed value |
| --- | --- |
| `market_type` | `total` or explicit binary type if book exposes NRFI/YRFI differently |
| `surface_key` | `first_inning_runs` |
| `period` | `first_inning` |
| selections | `over` / `under`, or normalized `yes` / `no` only if all books expose a binary no-run market safely |

Important safety question:

- If one book exposes NRFI/YRFI and another exposes first-inning total 0.5, we need to confirm they are exactly equivalent under settlement rules before matching them.

Implementation steps:

1. Use HARs to identify book-specific market names, IDs, periods, and line semantics.
2. Add parser mappings behind scan-only gating.
3. Add fixtures and parser tests.
4. Add detector snapshot telemetry for near misses.
5. Run scan-only and measure frequency, quote survival, and min-edge survival.
6. Only then consider live placement with `$5` soft cap.

## Second Planned Surface: Soccer Corners

Status: planned after `surface_key` exists and first surface spike proves the framework.

Why it is second:

- The expansion report rates soccer corners as one of the clearest surfaces.
- Corner settlement is more comparable across books than cards or player props.
- It gives the engine a non-score total surface that tests the value of `surface_key`.

Likely surface mapping:

| Concept | Proposed value |
| --- | --- |
| `market_type` | `total` or `spread`, depending on book market |
| `surface_key` | `soccer_corners` |
| `period` | `full_game`, `first_half`, or explicit period from book |
| selections | `over` / `under` for totals; team selections for corner handicaps |

Implementation steps:

1. Use HARs to identify DK/FD/PIN corner market names and IDs.
2. Confirm corners are corners taken, not awarded, across books.
3. Add explicit parser mappings for corner totals only first.
4. Leave corner handicaps out until totals are proven.
5. Run scan-only before any live execution.

## Later Candidate Surfaces

### Golf Head-To-Head Matchups

Status: not immediately planned.

Why it is attractive:

- Slower market movement than live team sports.
- Public evidence suggests mature matchup rules.

Why it is not first:

- Requires stronger event identity for individual players.
- Requires withdrawal, cut, tie, and reduced-round rule handling.
- May need settlement ingest before meaningful production use.

Next required evidence:

- HARs for DK, FD, and Pinnacle golf matchups.
- Three-book settlement matrix for withdrawal, tie, cut, and reduced-round treatment.

### F1 / Motorsport Binary Markets

Status: not immediately planned.

Why it is attractive:

- Market existence appears real across the books.

Why it is delayed:

- DNF, classification, sprint/race distinction, qualifying, and top-finish settlement rules need a full rule diff.

Next required evidence:

- HARs for driver matchup and top-finish markets.
- Three-book settlement matrix for classified finish, DNF, DNS, sprint, qualifying, and shortened race handling.

### UFC Goes Distance

Status: not immediately planned.

Why it is attractive:

- Two-outcome structure is simple.

Why it is delayed:

- Technical decision, doctor stoppage, DQ, no-contest, cancellation, and changed-round-count rules can diverge across books.

Next required evidence:

- HARs for goes-distance markets.
- Three-book settlement matrix before parser work.

### Mainstream Player Props

Status: roadmap only.

Why it is attractive:

- Large market menu.

Why it is delayed:

- Requires player identity, stat-source identity, correction-window policy, starter/participation rules, overtime rules, void rules, and settlement reconciliation.

Required framework before starting:

- `surface_key`.
- Strong `player_key`.
- Stat-source and correction-window matrix.
- Fixture harness.
- Settlement ingest decision.

### FanDuel Basketball Alternate Spreads

Status: roadmap only, after FD handicap flow is proven.

Why it is delayed:

- Alt spreads may be more fragile in the FD UI/API path.
- FanDuel may treat many alts as parlay-oriented surfaces.
- Current DK and Pinnacle scanners intentionally skip many alternate lines.

Next required evidence:

- FD NBA alt-spread HAR with `getMarketPrices` and `implyBets`.
- Confirmation that the market can be straight-bet validated and placed.
- Explicit decision about whether to add ladder/alt-line dedup before live use.

### Cards, Player Soccer Stats, Tennis, Esports, And Long-Tail Two-Way Surfaces

Status: not planned.

Why:

- These are too broad to implement as one bucket.
- Each needs its own settlement, identity, parser, and economics evaluation.

## Surface Readiness Matrix

| Surface | Market existence | Ontario/API evidence | Settlement confidence | Engine readiness | Current decision |
| --- | --- | --- | --- | --- | --- |
| Current game moneyline/spread/total | Proven in engine | Proven enough for current live testing | Known enough for current scope | Live | Keep |
| MLB first-inning total / NRFI-style | Likely | HARs pending | Needs rule confirmation | Needs `surface_key` and parser work | First spike |
| Soccer corners totals | Likely | HARs pending | Promising, needs confirmation | Needs `surface_key` and parser work | Second spike |
| Golf matchups | Likely | HARs not requested yet | Needs full matrix | Needs player matchup identity | Later |
| F1 / motorsport binaries | Likely | HARs not requested yet | Needs full matrix | Needs participant/race identity | Later |
| UFC goes distance | Likely | HARs not requested yet | Needs full matrix | Needs explicit binary surface | Later |
| Player props | Real but broad | HARs not requested yet | Needs stat-source matrix | Not ready | Roadmap |
| FD basketball alts | Real but execution uncertain | HARs not requested yet | Similar to spreads but API risk remains | FD handicap fix first | Roadmap |

## Economics And Telemetry Requirements

Before live-enabling a new surface, scan-only runs should measure:

- Number of markets fetched by book.
- Number of shared identity keys.
- Number of complementary pairs.
- Candidate count before min-edge.
- Candidate count after min-edge.
- Pinnacle quote survival rate.
- Soft validation survival rate.
- Average and max quoted Pinnacle stake.
- Soft-book max or rejection behavior.
- Family dedup impact.
- Latency from poll start to candidate execution decision.

Recommended telemetry additions:

- Include `surface_key` in detector diagnostics.
- Include `surface_key` in `ARB_CANDIDATE`, `MIN_EDGE_SKIPPED`, `FIRE_ENTER`, and live attempt events.
- Include surface in terminal `[ARB]` output when non-default.

## Open Decisions

| Decision | Current recommendation |
| --- | --- |
| Keep Pinnacle mandatory for expanded live markets? | Yes. |
| First implementation surface? | MLB first-inning total / NRFI-style. |
| First larger production expansion surface? | Soccer corners. |
| Initial live stake cap for expanded surfaces? | Keep `$5` soft cap. |
| Add settlement ingest before props? | Yes, or at least decide the correction-window policy first. |
| Add settlement ingest before golf top-finish? | Recommended before top-finish, less urgent for simple matchup scan-only. |
| Use guest Pinnacle odds for expansion? | No; keep authenticated Pinnacle scan as default. |
| Allow expanded markets live immediately after parser support? | No; scan-only first. |

## Working Checklist

Current:

- [ ] Collect requested HARs.
- [ ] Store HARs locally and keep raw files private.
- [ ] Review HARs for requested endpoints and field coverage.

Next:

- [ ] Sanitize HAR-derived fixtures.
- [ ] Fix FanDuel handicap propagation.
- [ ] Add `surface_key` to canonical market identity.
- [ ] Add `LIVE_ALLOWED_SURFACES` scan/live gating.
- [ ] Add fixture-backed parser tests.
- [ ] Implement MLB first-inning total / NRFI-style scan-only parsing.
- [ ] Run scan-only telemetry and review whether candidates are real.

Later:

- [ ] Implement soccer corners scan-only parsing.
- [ ] Build settlement rule matrix for golf, F1, UFC, cards, and props.
- [ ] Decide settlement ingest requirements.
- [ ] Add player/stat identity framework if props become active.
- [ ] Evaluate FD basketball alts after FD handicap path is proven.

## Definition Of Done For First Expansion Phase

The first expansion phase is done when:

- FD handicap propagation is fixed and verified.
- `surface_key` exists end-to-end for scan/detect/verify/telemetry/family dedup.
- `LIVE_ALLOWED_SURFACES` or equivalent prevents accidental live placement.
- MLB first-inning total / NRFI-style markets can be parsed from DK, FD, and Pinnacle fixtures.
- The engine can run scan-only with the new surface enabled.
- Terminal and JSONL telemetry clearly explain why candidates did or did not progress.
- No expanded surface is live-enabled by default.

