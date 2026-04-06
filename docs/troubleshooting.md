# Troubleshooting

## DraftKings

### All DK fetches fail with "error sending request"
**Cause**: Rust's reqwest can't complete TLS handshake with DK's CDN on Windows.
**Fix**: Start the DK proxy: `node dk-proxy.js`. The engine routes DK requests through it automatically.

### DK proxy hangs / no response
**Cause**: Previous version used Node's `https.get` which had the same TLS issue. Current version uses `fetch`.
**Fix**: Ensure you're running the latest `dk-proxy.js`. Kill stale Node processes: `Get-Process -Name "node" | Stop-Process -Force`

### "Could not authenticate" / 401 on calculateBets
**Cause**: DK session cookies expired.
**Fix**: Refresh the DraftKings browser window or re-login.

### "calculateBets failed (400)"
**Cause**: Invalid selection ID or market no longer available.
**Fix**: Engine will pick up fresh selection IDs on next scan cycle.

### Bet confirmation timeout after 15s
**Cause**: DK returned `waitingBetId` but bet never confirmed.
**Fix**: Check your DK account — bet may still be pending. Don't retry blindly.

### Spread/total bet placed but refunded (status: "NewOffer")
**Cause**: DK `placeBets` API requires `points` field in `selections` array for spread/total markets. Without it, DK accepts the bet but returns `NewOffer` instead of `Open`, then refunds within 2 minutes.
**Fix**: Fixed in `draftkings.js` — `points` from `calculateBets` response is now passed through to `placeBet`. DK minimum bet is ~$0.50; below this also gets refunded.

## FanDuel

### "placeBet failed (403): DSC-0015"
**Cause**: PerimeterX blocking the request.
**Fix**: This was fixed by using `context.request.post()` instead of `page.evaluate(fetch())`. If it recurs, refresh the FD browser window to get fresh PX cookies.

### "no auth token" / authToken is null
**Cause**: FD session expired (~13 min).
**Fix**: Re-login in the FD browser window. Tokens re-read on every request.

### "no geo token" / geoPacket is null
**Cause**: GeoComply PLC not running.
**Fix**: `taskkill /F /IM PlayerLocationCheck.exe` (auto-restarts), then refresh FD tab.

### "implyBets failed: NOT_STARTED" or "SUSPENDED"
**Cause**: Market not open for betting.
**Fix**: Wait or engine will find a different pair.

## Pinnacle

### "Missing header parameter 'X-Session'"
**Cause**: Adapter can't find session token in localStorage.
**Fix**: Ensure you're logged into Pinnacle in the browser window. Session is stored in `localStorage["Main:User"].token`.

### Quote returns different price than expected
**Cause**: Odds moved between detection and quote. This is normal — Pinnacle updates odds frequently.
**Fix**: Engine's odds drift check will reject if drift >3%.

### Bet acceptance timeout after 20s
**Cause**: Pinnacle's bet acceptance is asynchronous and can take up to 10s. If it exceeds 20s, something is wrong.
**Fix**: Check your Pinnacle account. The bet may have been accepted after the timeout.

### "quote failed (400)"
**Cause**: Market closed, invalid matchupId, or marketKey format wrong.
**Fix**: Engine will get fresh data on next scan cycle.

## Engine

### Arbs with "DRAW" selections (fake arbs)
**Cause**: Soccer/MLS moneylines are 3-way (win/draw/lose). Pairing "Team A wins" with "Draw" is not a valid 2-way arb — if Team B wins, both bets lose. These appeared as 40-70% arbs which were obviously too good to be true.
**Fix**: Two-layer fix:
1. `team_names.rs` — `normalize_selection()` now catches "Draw", "The Draw", "Tie" and normalizes them to lowercase `"draw"` before they reach the team name normalizer (which would turn them into `"DRAW"` or `"DRA"`).
2. `arb_detector.rs` — `are_selections_complementary()` excludes `"draw"` from moneyline pairing. Only team-vs-team pairs are valid 2-way arbs.

### 34-38% NHL spread arbs (fake arbs)
**Cause**: Spread markets have signed lines (+1.5 underdog, -1.5 favorite). The `MarketIdentityKey` grouped by signed `line_key`, so same-side bets from different venues ended up in separate groups. The arb detector saw different team names and paired them as if complementary — but they were the same side of the same spread.
**Fix**: `models.rs` — `line_key` now uses `l.abs()` for spread markets, grouping both sides together. The complementarity check then correctly pairs favorite vs underdog.

### Book drops from ~12,000 to ~700 entries after first scan
**Cause**: Diff-based update only called `book.update()` for changed entries. Unchanged entries kept stale timestamps and expired after 30 seconds.
**Fix**: Added `book.touch()` method that refreshes timestamps on unchanged entries without overwriting odds.

### "BELOW_MIN_BET_AMOUNT" on Pinnacle
**Cause**: Pinnacle has per-market minimum stakes (varies, ~$0.81 CAD typical).
**Fix**: Pre-flight quote system. The engine now quotes Pinnacle before placing any bets, gets the exact `minRiskStake`/`maxRiskStake`, and adjusts both legs' stakes to fit within limits while maintaining the arb ratio. If the adjusted total exceeds `MAX_STAKE`, the arb is skipped.

### Pinnacle alternate lines creating fake arbs
**Cause**: Pinnacle returns alternate spread/total lines (`isAlternate: true`) at different odds than the main line. These were being paired against FD/DK main lines, creating impossible 34-38% "arbs."
**Fix**: `odds_fetcher.rs` now filters out `isAlternate: true` Pinnacle markets. Only main lines are imported.

### "failed to open sqlite db"
**Cause**: `data/` directory doesn't exist relative to where the engine runs.
**Fix**: `mkdir C:\dev\sportsbook-arbitrage\engine\data`

### "WebSocket connection failed — falling back to paper"
**Cause**: Bet placer not running when engine starts.
**Fix**: Start the bet placer first, then the engine.

### No arbs detected
**Cause**: Normal — true arbs are rare between two soft books (DK/FD). Most arbs involve Pinnacle.
**Fix**: Keep running. The engine scans every 5s. Pinnacle arbs are more frequent.

### "[HEALTH] draftkings — no data"
**Cause**: DK proxy not running, or DK API returning errors for that sport (offseason).
**Fix**: Start DK proxy. Offseason sports returning empty data is expected.

### "[ODDS DRIFT]" skipping trade
**Cause**: Odds moved >3% between detection and execution attempt.
**Fix**: Normal safety behavior — prevents placing bets on stale prices.

### "[RATE LIMIT]" skipping trade
**Cause**: Hit the per-venue bet limit.
**Fix**: Wait for the sliding window to clear, or adjust rate limit settings in the engine code.

## Browser / Patchright

### "browser not launched"
**Cause**: Browser window was closed manually or crashed.
**Fix**: Restart the bet placer.

### Browser profile corrupted
**Fix**: Delete the profile and re-login:
```powershell
Remove-Item -Recurse -Force C:\dev\sportsbook-arbitrage\data\profiles\draftkings
Remove-Item -Recurse -Force C:\dev\sportsbook-arbitrage\data\profiles\fanduel
Remove-Item -Recurse -Force C:\dev\sportsbook-arbitrage\data\profiles\pinnacle
```
Then restart the bet placer and log in again.

### Sessions expire during long runs
**Cause**: Keepalive refreshes pages every 5 min, but sessions can still expire.
**Fix**: Re-login in the browser windows. Verify with `balance` command.

## Common Patterns

### One-sided exposure (leg 1 placed, leg 2 failed)
You have a live bet on one book with no hedge. Options:
1. Manually place the other leg via `bet` command
2. Cash out the placed bet (if available on that book)
3. Accept the risk

### Engine exits with STATUS_CONTROL_C_EXIT
**Not an error** — normal Windows behavior when you press Ctrl+C.
