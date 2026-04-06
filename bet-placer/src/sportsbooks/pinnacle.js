const crypto = require("crypto");

const API_BASE = "https://api.arcadia.pinnacle.com/0.1";
const X_API_KEY = "CmX2KcMrXuFmNg6YFbmTxE0y9CIrOi0R";
const TOKEN_REFRESH_MS = 5 * 60 * 1000; // Re-read tokens from browser every 5 min

class PinnacleAdapter {
  constructor(browserManager) {
    this.browserManager = browserManager;
    this.bookId = "pinnacle";
    this.deviceUuid = null;
    this.session = null;
    this.username = null;
    this._lastTokenRefresh = 0;
  }

  _getPage() {
    const page = this.browserManager.getPage(this.bookId);
    if (!page) throw new Error("Pinnacle browser not launched");
    return page;
  }

  async _refreshTokens() {
    const now = Date.now();
    if (this.session && now - this._lastTokenRefresh < TOKEN_REFRESH_MS) return;

    const page = this._getPage();
    const user = await page.evaluate(`
      (() => {
        try {
          const u = JSON.parse(localStorage.getItem("Main:User") || "{}");
          return { deviceUuid: u.deviceId || null, session: u.token || null, username: u.username || null };
        } catch(e) { return { deviceUuid: null, session: null, username: null }; }
      })()
    `);

    this.deviceUuid = user.deviceUuid;
    this.session = user.session;
    this.username = user.username;
    this._lastTokenRefresh = now;
  }

  async _apiRequest(url, options = {}) {
    const { method = "GET", body = null } = options;
    await this._refreshTokens();

    const headers = {
      "content-type": "application/json",
      "accept": "application/json",
      "x-api-key": X_API_KEY,
      "origin": "https://www.pinnacle.ca",
      "referer": "https://www.pinnacle.ca/",
    };

    if (this.deviceUuid) headers["x-device-uuid"] = this.deviceUuid;
    if (this.session) headers["x-session"] = this.session;

    const fetchOptions = { method, headers };
    if (body) fetchOptions.body = JSON.stringify(body);

    const resp = await fetch(url, fetchOptions);
    const text = await resp.text();

    return {
      status: resp.status,
      headers: Object.fromEntries(resp.headers.entries()),
      body: text,
      json: () => JSON.parse(text),
    };
  }

  async getBalance() {
    await this._refreshTokens();
    const resp = await this._apiRequest(`${API_BASE}/wallet/balance`);
    if (resp.status !== 200) throw new Error(`getBalance failed (${resp.status}): ${resp.body}`);
    const data = resp.json();
    return {
      username: this.username || "pinnacle_user",
      accounts: [{ currencyCode: data.currency || "CAD", balance: data.amount || 0 }],
    };
  }

  async getSelectionInfo(selectionId) {
    const [matchupId, marketKey, designation] = selectionId.split(":");
    const resp = await this._apiRequest(`${API_BASE}/matchups/${matchupId}/market/${encodeURIComponent(marketKey)}`);
    if (resp.status !== 200) throw new Error(`getMarket failed (${resp.status}): ${resp.body}`);
    const market = resp.json();
    const price = market.prices?.find(p => p.designation === designation);
    return {
      events: [{ betSlipLine: `Matchup ${matchupId}` }],
      markets: [{ id: marketKey, betSlipLine: market.type || marketKey }],
      selections: [{
        id: selectionId,
        marketId: marketKey,
        betSlipLine: designation,
        trueOdds: price ? (price.price > 0 ? 1 + price.price / 100 : 1 + 100 / Math.abs(price.price)) : 0,
        displayOdds: { american: price?.price?.toString() || "?" },
      }],
    };
  }

  async executeBet(selectionId, stake) {
    const parts = selectionId.split(":");
    const matchupId = parseInt(parts[0]);
    const marketKey = parts.slice(1, -1).join(":");
    const designation = parts[parts.length - 1];

    // Step 1: Quote
    console.log(`[PIN] Quoting: matchup=${matchupId} market=${marketKey} side=${designation}`);
    const quoteResp = await this._apiRequest(`${API_BASE}/bets/straight/quote`, {
      method: "POST",
      body: {
        oddsFormat: "american",
        selections: [{ matchupId, marketKey, designation, price: 0 }],
      },
    });
    if (quoteResp.status !== 200) throw new Error(`quote failed (${quoteResp.status}): ${quoteResp.body}`);
    const quote = quoteResp.json();

    const sel = quote.selections?.[0];
    if (!sel) throw new Error("No selection in quote response");
    const marketId = sel.marketId;
    const price = sel.price;

    const decimal = price > 0 ? 1 + price / 100 : 1 + 100 / Math.abs(price);
    console.log(`[PIN] Quote: marketId=${marketId} price=${price} (${decimal.toFixed(3)})`);

    // Step 2: Place
    const requestId = crypto.randomUUID();
    console.log(`[PIN] Placing $${stake} bet...`);
    const placeResp = await this._apiRequest(`${API_BASE}/bets/straight`, {
      method: "POST",
      body: {
        oddsFormat: "american",
        requestId,
        acceptBetterPrice: true,
        class: "Straight",
        selections: [{ marketId, matchupId, marketKey, designation, price }],
        stake,
        originTag: "sl:bsd",
      },
    });
    if (placeResp.status !== 200) throw new Error(`placeBet failed (${placeResp.status}): ${placeResp.body}`);

    // Step 3: Poll for acceptance
    console.log(`[PIN] Polling for acceptance...`);
    const start = Date.now();
    while (Date.now() - start < 20000) {
      await new Promise(r => setTimeout(r, 1500));
      const pollResp = await this._apiRequest(`${API_BASE}/bets?requestIds=${requestId}`);
      if (pollResp.status !== 200) continue;
      const data = pollResp.json();
      const bet = data.bets?.[0];
      if (bet && bet.status !== "pending") {
        console.log(`[PIN] Bet accepted! id=${bet.id} stake=$${bet.stake} toWin=$${bet.toWin}`);
        return {
          status: bet.status,
          bets: [{
            type: "Single",
            stake: bet.stake,
            displayOdds: String(bet.price),
            potentialReturns: bet.stake + (bet.toWin || 0),
            status: bet.status,
            betId: bet.id,
          }],
        };
      }
    }
    throw new Error("Bet acceptance timed out after 20s");
  }

  async saveCookies() {
    await this.browserManager.saveCookies(this.bookId);
  }
}

module.exports = { PinnacleAdapter };
