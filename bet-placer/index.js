// ═══════════════════════════════════════════════════════════════════
// Bet Placer — WebSocket server bridging Rust engine to
// Patchright browser automation for live bet placement
// ═══════════════════════════════════════════════════════════════════
const readline = require("readline");
const { WebSocketServer } = require("ws");
const { BrowserManager } = require("./src/browser-manager");
const { SPORTSBOOKS } = require("./src/sportsbooks");
const { DraftKingsAdapter } = require("./src/sportsbooks/draftkings");
const { FanDuelAdapter } = require("./src/sportsbooks/fanduel");
const { PinnacleAdapter } = require("./src/sportsbooks/pinnacle");

const WS_PORT = parseInt(process.env.WS_PORT || "3402", 10);
const KEEPALIVE_INTERVAL_MS = 5 * 60 * 1000;

async function main() {
  console.log("╔═══════════════════════════════════════════════════════════╗");
  console.log("║   ARB BET PLACER — Patchright + WebSocket Bridge         ║");
  console.log("╚═══════════════════════════════════════════════════════════╝");

  const manager = new BrowserManager();
  const adapters = {};

  // ── Launch browsers ──
  console.log("\nLaunching DraftKings...");
  await manager.launch("draftkings");
  await manager.navigateTo("draftkings", SPORTSBOOKS.draftkings.url);

  console.log("Launching FanDuel...");
  await manager.launch("fanduel");
  await manager.navigateTo("fanduel", SPORTSBOOKS.fanduel.url);

  console.log("Launching Pinnacle...");
  await manager.launch("pinnacle");
  await manager.navigateTo("pinnacle", SPORTSBOOKS.pinnacle.url);

  // ── Initialize adapters ──
  const dk = new DraftKingsAdapter(manager);
  adapters.draftkings = dk;

  const fd = new FanDuelAdapter(manager);
  adapters.fanduel = fd;

  const pin = new PinnacleAdapter(manager);
  adapters.pinnacle = pin;

  // ── Verify sessions ──
  console.log("\nChecking sessions...");
  for (const [bookId, adapter] of Object.entries(adapters)) {
    try {
      const balance = await adapter.getBalance();
      console.log(`  [${bookId}] Logged in as: ${balance.username}`);
      for (const acct of balance.accounts) {
        if (acct.balance > 0) {
          console.log(`    ${acct.currencyCode}: $${acct.balance.toFixed(2)}`);
        }
      }
    } catch (err) {
      console.error(`  [${bookId}] Session check failed: ${err.message}`);
      console.log(`  -> Please log in to ${bookId} in the browser window.`);
    }
  }

  // ── Session keepalive ──
  const keepaliveTimer = setInterval(async () => {
    for (const bookId of manager.listRunning()) {
      try {
        const page = manager.getPage(bookId);
        await page.reload({ waitUntil: "domcontentloaded" });
        console.log(`[keepalive] ${bookId} refreshed`);
      } catch (err) {
        console.error(`[keepalive] ${bookId} failed: ${err.message}`);
      }
    }
  }, KEEPALIVE_INTERVAL_MS);

  // ═══════════════════════════════════════════════════════════════
  // WebSocket Server — receives commands from Rust engine
  // ═══════════════════════════════════════════════════════════════
  const wss = new WebSocketServer({ port: WS_PORT });
  let engineSocket = null;

  wss.on("connection", (ws) => {
    console.log(`[WS] Engine connected`);
    engineSocket = ws;

    ws.on("message", async (raw) => {
      let msg;
      try {
        msg = JSON.parse(raw.toString());
      } catch (e) {
        console.error("[WS] Invalid JSON:", raw.toString().substring(0, 200));
        return;
      }

      if (msg.type === "place_bet") {
        await handlePlaceBet(ws, msg);
      } else if (msg.type === "quote_bet") {
        await handleQuoteBet(ws, msg);
      } else if (msg.type === "get_balances") {
        await handleGetBalances(ws);
      } else if (msg.type === "ping") {
        ws.send(JSON.stringify({ type: "pong" }));
      } else {
        console.log(`[WS] Unknown message type: ${msg.type}`);
      }
    });

    ws.on("close", () => {
      console.log("[WS] Engine disconnected");
      if (engineSocket === ws) engineSocket = null;
    });

    ws.on("error", (err) => {
      console.error("[WS] Error:", err.message);
    });
  });

  console.log(`\n[WS] Listening on ws://localhost:${WS_PORT}`);

  // ── Handle quote_bet request (Pinnacle only) ──
  async function handleQuoteBet(ws, cmd) {
    const { attempt_id, venue, venue_market_id } = cmd;
    console.log(`[QUOTE] ${venue} | ${venue_market_id}`);

    if (venue !== "pinnacle") {
      ws.send(JSON.stringify({ type: "quote_result", attempt_id, status: "ok", min_stake: 0, max_stake: 999999, price: 0 }));
      return;
    }

    const adapter = adapters[venue];
    if (!adapter) {
      ws.send(JSON.stringify({ type: "quote_result", attempt_id, status: "error", error_message: `Unknown venue: ${venue}` }));
      return;
    }

    try {
      const parts = venue_market_id.split(":");
      const matchupId = parseInt(parts[0]);
      const marketKey = parts.slice(1, -1).join(":");
      const designation = parts[parts.length - 1];

      const quoteResp = await adapter._apiRequest("https://api.arcadia.pinnacle.com/0.1/bets/straight/quote", {
        method: "POST",
        body: { oddsFormat: "american", selections: [{ matchupId, marketKey, designation, price: 0 }] },
      });

      if (quoteResp.status !== 200) throw new Error(`quote failed (${quoteResp.status}): ${quoteResp.body}`);
      const quote = quoteResp.json();
      const sel = quote.selections?.[0];
      if (!sel) throw new Error("No selection in quote");

      const limits = quote.limits || [];
      const minStake = limits.find(l => l.type === "minRiskStake")?.amount || 0;
      const maxStake = limits.find(l => l.type === "maxRiskStake")?.amount || 999999;
      const price = sel.price;
      const decimal = price > 0 ? 1 + price / 100 : 1 + 100 / Math.abs(price);

      console.log(`[QUOTE] PIN min=$${minStake} max=$${maxStake} price=${price} (${decimal.toFixed(3)})`);
      ws.send(JSON.stringify({ type: "quote_result", attempt_id, status: "ok", min_stake: minStake, max_stake: maxStake, price: decimal }));
    } catch (err) {
      console.error(`[QUOTE] ${venue} failed: ${err.message}`);
      ws.send(JSON.stringify({ type: "quote_result", attempt_id, status: "error", error_message: err.message }));
    }
  }

  // ── Handle get_balances request ──
  async function handleGetBalances(ws) {
    console.log("[WS] Balance request received");
    const balances = [];
    for (const [venue, adapter] of Object.entries(adapters)) {
      try {
        const bal = await adapter.getBalance();
        const total = bal.accounts.reduce((sum, a) => sum + a.balance, 0);
        balances.push({ venue, balance: total });
        console.log(`  [${venue}] $${total.toFixed(2)}`);
      } catch (err) {
        console.error(`  [${venue}] balance fetch failed: ${err.message}`);
        balances.push({ venue, balance: 0 });
      }
    }
    ws.send(JSON.stringify({ type: "balances", balances }));
  }

  // ── Handle place_bet command ──
  async function handlePlaceBet(ws, cmd) {
    const { attempt_id, venue, venue_market_id, selection, stake, expected_odds } = cmd;
    console.log(`\n[BET] ${attempt_id} | ${venue} | ${venue_market_id} | ${selection} | $${stake} @ ${expected_odds}`);

    const adapter = adapters[venue];
    if (!adapter) {
      const report = {
        type: "fill_report",
        attempt_id,
        status: "error",
        accepted_stake: null,
        accepted_odds: null,
        error_message: `Unknown venue: ${venue}`,
      };
      ws.send(JSON.stringify(report));
      return;
    }

    try {
      const result = await adapter.executeBet(venue_market_id, stake);
      const bet = result.bets?.[0];

      // Extract accepted odds from the result
      let acceptedOdds = expected_odds;
      if (bet?.displayOdds) {
        // Try to parse decimal from the result
        const american = parseFloat(String(bet.displayOdds).replace("+", ""));
        if (!isNaN(american)) {
          acceptedOdds = american > 0
            ? 1 + american / 100
            : 1 + 100 / Math.abs(american);
        }
      }

      const report = {
        type: "fill_report",
        attempt_id,
        status: "filled",
        accepted_stake: stake,
        accepted_odds: acceptedOdds,
        error_message: null,
      };

      console.log(`[BET] ${attempt_id} FILLED | ${venue} | $${stake} @ ${acceptedOdds.toFixed(3)}`);
      ws.send(JSON.stringify(report));

      // Also send balance update
      try {
        const bal = await adapter.getBalance();
        const totalBalance = bal.accounts.reduce((sum, a) => sum + a.balance, 0);
        ws.send(JSON.stringify({
          type: "balance_update",
          venue,
          balance: totalBalance,
        }));
      } catch (_) {
        // Non-critical — balance update is best-effort
      }
    } catch (err) {
      console.error(`[BET] ${attempt_id} FAILED | ${venue} | ${err.message}`);

      const report = {
        type: "fill_report",
        attempt_id,
        status: "rejected",
        accepted_stake: null,
        accepted_odds: null,
        error_message: err.message,
      };
      ws.send(JSON.stringify(report));
    }
  }

  // ═══════════════════════════════════════════════════════════════
  // CLI — manual commands for testing/monitoring
  // ═══════════════════════════════════════════════════════════════
  const rl = readline.createInterface({ input: process.stdin, output: process.stdout });
  const ask = (q) => new Promise((resolve) => rl.question(q, resolve));

  console.log("\n--- Commands ---");
  console.log("  balance [book]   — Check balance");
  console.log("  info <book> <id> — Get selection info");
  console.log("  bet <book> <id> <stake> — Place single bet (REAL MONEY)");
  console.log("  status           — Show WS connection status");
  console.log("  quit\n");

  let running = true;
  while (running) {
    const input = await ask("> ");
    const parts = input.trim().split(/\s+/);
    const cmd = parts[0]?.toLowerCase();

    try {
      switch (cmd) {
        case "balance": {
          const bookId = parts[1];
          const books = bookId ? [bookId] : Object.keys(adapters);
          for (const id of books) {
            const bal = await adapters[id].getBalance();
            console.log(`[${id}] ${bal.username}`);
            for (const acct of bal.accounts) {
              if (acct.balance > 0) {
                console.log(`  ${acct.currencyCode}: $${acct.balance.toFixed(2)}`);
              }
            }
          }
          break;
        }

        case "info": {
          const [, bookId, selId] = parts;
          if (!bookId || !selId) { console.log("Usage: info <book> <selectionId>"); break; }
          const adapter = adapters[bookId];
          if (!adapter) { console.log(`Unknown book: ${bookId}`); break; }
          const info = await adapter.getSelectionInfo(selId);
          for (const sel of info.selections) {
            const market = info.markets?.find((m) => m.id === sel.marketId);
            const event = info.events?.[0];
            console.log(`  Event: ${event?.betSlipLine}`);
            console.log(`  Market: ${market?.betSlipLine}`);
            console.log(`  Selection: ${sel.betSlipLine}`);
            console.log(`  Odds: ${sel.displayOdds?.american} (decimal: ${sel.trueOdds})`);
          }
          break;
        }

        case "bet": {
          const [, bookId, selId, stakeStr] = parts;
          if (!bookId || !selId || !stakeStr) { console.log("Usage: bet <book> <selectionId> <stake>"); break; }
          const adapter = adapters[bookId];
          if (!adapter) { console.log(`Unknown book: ${bookId}`); break; }
          const result = await adapter.executeBet(selId, parseFloat(stakeStr));
          const bet = result.bets?.[0];
          console.log(`Placed: $${bet?.stake} @ ${bet?.displayOdds} -> $${bet?.potentialReturns} | Status: ${result.status}`);
          break;
        }

        case "status": {
          console.log(`  Engine WS: ${engineSocket ? "connected" : "disconnected"}`);
          console.log(`  Browsers: ${manager.listRunning().join(", ") || "none"}`);
          break;
        }

        case "quit":
        case "exit":
          running = false;
          break;

        default:
          if (cmd) console.log(`Unknown command: ${cmd}`);
          break;
      }
    } catch (err) {
      console.error(`Error: ${err.message}`);
    }
  }

  clearInterval(keepaliveTimer);
  wss.close();
  for (const [, adapter] of Object.entries(adapters)) {
    await adapter.saveCookies();
  }
  rl.close();
  await manager.closeAll();
  console.log("Done.");
}

main().catch((err) => {
  console.error("Fatal error:", err);
  process.exit(1);
});
