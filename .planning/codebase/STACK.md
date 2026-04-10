# Technology Stack

**Analysis Date:** 2026-04-09

## Languages

**Primary:**
- Rust 1.94.0 — Arb detection engine (`engine/src/`)
- JavaScript (Node.js) — Bet placer and DK proxy (`bet-placer/`, `dk-proxy.js`)

**Secondary:**
- SQL — SQLite schema and queries (`engine/src/db.rs`)
- TOML — Engine configuration (`engine/config.toml`, `engine/Cargo.toml`)

## Runtime

**Environment:**
- Node.js v24.13.0 — bet-placer and dk-proxy
- Rust 1.94.0 / Cargo 1.94.0 — engine (2021 edition)

**Package Manager:**
- Cargo — Rust dependencies, lockfile at `engine/Cargo.lock`
- npm — Node.js dependencies, lockfile at `bet-placer/package-lock.json`

## Frameworks

**Core (Rust):**
- axum 0.7 (with `ws` feature) — HTTP + WebSocket server on port 3401 (`engine/src/main.rs`)
- tokio 1.x (full features) — async runtime for all concurrent polling tasks
- tower-http 0.5 (with `cors`) — CORS middleware on the REST API

**Browser Automation (Node.js):**
- patchright 1.59.0 — Chromium automation with stealth patches (anti-bot bypass). Wraps Playwright's `chromium`. Used in `bet-placer/src/browser-manager.js`.

**WebSocket (Node.js):**
- ws 8.18.0 — WebSocket server on port 3402 in `bet-placer/index.js`

**Build/Dev:**
- cargo — build, run, test (`cargo run` in `engine/`)
- node — direct execution (`node index.js`, `node dk-proxy.js`)

## Key Dependencies

**Critical (Rust):**
- `reqwest` 0.12 (`json`, `native-tls`, `gzip`, `brotli`, `deflate`) — all outbound HTTP requests to DK proxy, FanDuel, Pinnacle odds APIs (`engine/src/odds_fetcher.rs`)
- `tokio-tungstenite` 0.24 (`native-tls`) — outbound WebSocket client connecting engine to bet placer (`engine/src/live_executor.rs`)
- `rusqlite` 0.31 (`bundled`) — embedded SQLite for trades, arbs, balances, settings. Bundled = no external sqlite install needed. (`engine/src/db.rs`)
- `serde` / `serde_json` 1.x — all JSON serialization/deserialization throughout engine
- `axum-extra` 0.9 (`typed-header`) — typed header extraction in HTTP handlers

**Infrastructure (Rust):**
- `tracing` 0.1 + `tracing-subscriber` 0.3 (`env-filter`, `fmt`) — structured logging with `info`/`warn`/`debug` levels throughout engine
- `chrono` 0.4 (`serde`) — RFC3339 timestamps, time-to-start calculations
- `uuid` 1.x (`v4`, `serde`) — UUIDs for trade/arb IDs
- `futures` 0.3 — stream combinators for WebSocket handling
- `rand` 0.8 — paper trading simulation (random fill/rejection)

**Critical (Node.js):**
- `patchright` 1.59.0 — stealth Chromium for all three sportsbook browsers
- `ws` 8.18.0 — WebSocket server receiving bet commands from Rust engine

## Configuration

**Runtime (Engine):**
- Environment variables override all settings at startup (see `engine/src/main.rs` lines 47-74)
- Settings also persisted in SQLite (`settings` table) and readable/writable via REST API
- Key env vars: `LIVE_MODE`, `FIXED_STAKE`, `MAX_STAKE`, `MIN_ARB_PCT`, `STAKE_MODE`, `PROPORTIONAL_PCT`, `ENABLED`, `MAX_HOURS_TO_START`, `BALANCE_REFRESH_CYCLES`, `SKIP_LIVE`, `WS_URL`

**File-based (Engine):**
- `engine/config.toml` — policy, paper trading, circuit breaker, concurrency, per-venue stake limits
- `engine/src/config.rs` — sport/league IDs, API hostnames, poll interval constants (hardcoded)

**Runtime (Bet Placer):**
- `WS_PORT` env var — WebSocket server port (default 3402)
- `DK_PROXY_PORT` env var — DK proxy port (default 3403)
- No `.env` files found — all config via shell env vars

**Build:**
- `engine/Cargo.toml` — Rust workspace/dependency manifest
- `bet-placer/package.json` — Node.js dependency manifest

## Platform Requirements

**Development:**
- Windows (primary — TLS issue with DK requires `dk-proxy.js` workaround)
- GeoComply PLC must be running for FanDuel geo-verification
- Chrome browser installed (patchright uses `channel: "chrome"`)
- Ontario, CA region

**Production:**
- Same Windows requirements apply in live mode
- Three persistent browser windows (DK, FD, PIN) kept open with authenticated sessions
- Profiles stored at `data/profiles/{bookId}/`, cookies at `data/cookies/{bookId}-data.json`
- SQLite DB at `data/trades.db`

---

*Stack analysis: 2026-04-09*
