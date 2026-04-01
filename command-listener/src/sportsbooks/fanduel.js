const fs = require("fs");
const path = require("path");
const crypto = require("crypto");

const COOKIES_DIR = path.join(__dirname, "..", "..", "..", "data", "cookies");

// FanDuel API hosts (Ontario region — adjust prefix for other regions)
const HOSTS = {
  imply: "https://sib.on.sportsbook.fanduel.ca",
  place: "https://spb.on.sportsbook.fanduel.ca",
  prices: "https://smp.on.sportsbook.fanduel.ca",
  account: "https://api.fanduel.ca",
  origin: "https://on.sportsbook.fanduel.ca",
};

const X_APPLICATION = "FhMFpcPWXMeyZxOx";

class FanDuelAdapter {
  /**
   * @param {import('../browser-manager').BrowserManager} browserManager
   */
  constructor(browserManager) {
    this.browserManager = browserManager;
    this.bookId = "fanduel";
    this._cachedTokens = null;
  }

  _getPage() {
    const page = this.browserManager.getPage(this.bookId);
    if (!page) throw new Error("FanDuel browser not launched");
    return page;
  }

  _getContext() {
    const entry = this.browserManager.browsers.get(this.bookId);
    if (!entry) throw new Error("FanDuel browser not launched");
    return entry.context;
  }

  /**
   * Extract auth tokens from the browser page (localStorage + cookies).
   */
  async _getTokens() {
    const page = this._getPage();

    const tokens = await page.evaluate(`
      (() => {
        // Auth token is in session-sportsbook JSON (authToken can be a string or { jwt: "..." })
        let authToken = null;
        try {
          const session = JSON.parse(localStorage.getItem("session-sportsbook") || "{}");
          const raw = session.authToken;
          if (typeof raw === "string") authToken = raw;
          else if (raw && typeof raw === "object" && raw.jwt) authToken = raw.jwt;
        } catch(e) {}

        // Geo packet is in location_token JSON
        let geoPacket = null;
        try {
          const loc = JSON.parse(localStorage.getItem("location_token") || "{}");
          geoPacket = loc.jwt || null;
        } catch(e) {}

        return { authToken, geoPacket, pxContext: "__FROM_COOKIES__" };
      })()
    `);

    this._cachedTokens = tokens;
    return tokens;
  }

  /**
   * Make an API request using Patchright's context.request (Node-side HTTP).
   * Bypasses CORS entirely. Sends cookies from the browser context automatically.
   */
  async _apiRequest(url, options = {}) {
    const { method = "GET", body = null, extraHeaders = {} } = options;
    const context = this._getContext();
    const tokens = await this._getTokens();

    // Build PX context from browser cookies (reads HttpOnly cookies)
    const allCookies = await context.cookies(HOSTS.origin);
    const px3 = allCookies.find((c) => c.name === "_px3");
    const pxvid = allCookies.find((c) => c.name === "_pxvid");
    const pxcts = allCookies.find((c) => c.name === "pxcts");
    let pxContext = "";
    if (px3) {
      pxContext = `_px3=${px3.value};`;
      if (pxvid) pxContext += `_pxvid=${pxvid.value};`;
      if (pxcts) pxContext += `pxcts=${pxcts.value};`;
    }

    const headers = {
      "content-type": "application/json",
      accept: "application/json",
      "x-application": X_APPLICATION,
      origin: HOSTS.origin,
      referer: HOSTS.origin + "/",
    };

    if (tokens.authToken && typeof tokens.authToken === "string") {
      headers["x-authentication"] = tokens.authToken;
      headers["x-auth-token"] = tokens.authToken;
    }
    if (pxContext.length > 0) {
      headers["x-px-context"] = pxContext;
    }
    if (tokens.geoPacket && typeof tokens.geoPacket === "string") {
      headers["x-geo-packet"] = tokens.geoPacket;
    }

    for (const [k, v] of Object.entries(extraHeaders)) {
      if (typeof v === "string") headers[k] = v;
    }


    const requestOptions = { headers };
    if (body) {
      requestOptions.data = body;
    }

    let resp;
    if (method === "POST") {
      resp = await context.request.post(url, requestOptions);
    } else {
      resp = await context.request.get(url, requestOptions);
    }

    const text = await resp.text();
    const responseHeaders = resp.headers();

    return {
      status: resp.status(),
      headers: responseHeaders,
      body: text,
      json: () => JSON.parse(text),
    };
  }

  /**
   * Get current odds for a market.
   * @param {string} marketId e.g. "801.161028546"
   */
  async getMarketPrices(marketId) {
    const url = `${HOSTS.prices}/api/sports/fixedodds/readonly/v1/getMarketPrices?priceHistory=1`;
    const resp = await this._apiRequest(url, {
      method: "POST",
      body: { marketIds: [marketId] },
    });
    if (resp.status !== 200) {
      throw new Error(`getMarketPrices failed (${resp.status}): ${resp.body}`);
    }
    const data = resp.json();
    // Response is an array of markets, normalize to { markets: [...] }
    const markets = Array.isArray(data) ? data : data.markets || [data];
    // Normalize runnerDetails → runners
    for (const m of markets) {
      if (m.runnerDetails && !m.runners) {
        m.runners = m.runnerDetails;
      }
    }
    return { markets };
  }

  /**
   * Get selection info — returns market with runner and odds.
   * @param {string} selectionId format: "marketId:selectionId" e.g. "801.161028546:7017906"
   */
  async getSelectionInfo(selectionId) {
    const [marketId, runnerId] = selectionId.split(":");
    if (!marketId || !runnerId) {
      throw new Error(`Invalid selection ID format. Expected "marketId:selectionId", got "${selectionId}"`);
    }
    const prices = await this.getMarketPrices(marketId);
    const market = prices.markets?.[0];
    if (!market) throw new Error(`Market ${marketId} not found`);

    const runner = market.runners?.find((r) => String(r.selectionId) === runnerId);
    if (!runner) throw new Error(`Runner ${runnerId} not found in market ${marketId}`);

    return {
      events: [{ betSlipLine: market.marketName || marketId }],
      markets: [{ id: marketId, betSlipLine: market.marketType || "Unknown" }],
      selections: [
        {
          id: selectionId,
          marketId,
          selectionId: parseInt(runnerId),
          betSlipLine: runner.runnerName || `Runner ${runnerId}`,
          trueOdds: runner.winRunnerOdds?.trueOdds?.decimalOdds?.decimalOdds || 0,
          displayOdds: {
            american: runner.winRunnerOdds?.americanDisplayOdds?.americanOdds || "?",
            decimal: runner.winRunnerOdds?.trueOdds?.decimalOdds?.decimalOdds || 0,
          },
          handicap: runner.handicap,
        },
      ],
    };
  }

  /**
   * Step 1: implyBets — validate bet and get betReference token.
   */
  async implyBets(marketId, selectionId, handicap) {
    const url = `${HOSTS.imply}/api/sports/fixedodds/transactional/v1/implyBets?pricePolicy=SUGGESTED`;

    const runner = { marketId, selectionId, handicap: handicap ?? 0 };

    const body = {
      betLegs: [
        {
          legType: "SIMPLE_SELECTION",
          betRunners: [{ runner }],
        },
      ],
    };

    const resp = await this._apiRequest(url, { method: "POST", body });

    if (resp.status !== 200) {
      throw new Error(`implyBets failed (${resp.status}): ${resp.body}`);
    }
    return resp.json();
  }

  /**
   * Step 2: placeBet — submit the bet.
   */
  async placeBet({
    marketId,
    selectionId,
    handicap,
    stake,
    betReference,
    decimalOdds,
    fractionalNumerator,
    fractionalDenominator,
  }) {
    const url = `${HOSTS.place}/api/sports/fixedodds/transactional/v1/placeBet`;

    const runner = { marketId, selectionId, handicap: handicap ?? 0 };

    const body = {
      customerRef: crypto.randomUUID(),
      acceptLowerOdds: false,
      useAvailableBonus: false,
      dryRun: false,
      betDefinitions: [
        {
          betReference,
          betType: "SINGLE",
          betNo: 0,
          stakePerLine: stake,
          winExpectedOdds: {
            trueOdds: {
              decimalOdds: { decimalOdds },
            },
          },
          edges: [],
          legs: [
            {
              legType: "SIMPLE_SELECTION",
              betRunners: [{ runner }],
              winExpectedOdds: {
                fractionalOdds: {
                  numerator: fractionalNumerator,
                  denominator: fractionalDenominator,
                },
              },
              edgeContexts: [],
            },
          ],
          wallets: [],
        },
      ],
    };


    // Use context.request.post (Node-side HTTP) — same as implyBets
    // This bypasses CORS and sends PX/auth headers directly
    const resp = await this._apiRequest(url, { method: "POST", body });

    if (resp.status !== 200) {
      throw new Error(`placeBet failed (${resp.status}): ${resp.body}`);
    }
    return resp.json();
  }

  /**
   * Get account balance by scraping it from the page.
   */
  async getBalance() {
    const page = this._getPage();

    const data = await page.evaluate(`
      (() => {
        let username = "fanduel_user";
        let balance = 0;
        try {
          const session = JSON.parse(localStorage.getItem("session-sportsbook") || "{}");
          // Username is in the JWT payload as "usn"
          const token = session.token || (session.authToken && (typeof session.authToken === "string" ? session.authToken : session.authToken.jwt));
          if (token) {
            try {
              const payload = JSON.parse(atob(token.split(".")[1]));
              if (payload.usn) username = payload.usn;
            } catch(e) {}
          }
        } catch(e) {}

        // Scrape balance from the page
        const allEls = document.querySelectorAll("span, div, p, button");
        for (const el of allEls) {
          const text = el.textContent.trim();
          if (text.match(/^\\$[0-9,.]+$/) && el.children.length === 0) {
            const val = parseFloat(text.replace(/[^0-9.]/g, ""));
            if (val > balance) balance = val;
          }
        }

        return { username, balance };
      })()
    `);

    return {
      username: data.username,
      accounts: [
        { currencyCode: "CAD", balance: data.balance, productBalance: data.balance },
      ],
    };
  }

  /**
   * Full bet placement flow: implyBets → placeBet.
   * @param {string} selectionIdStr format: "marketId:selectionId" e.g. "801.161028546:7017906"
   * @param {number} stake
   */
  async executeBet(selectionIdStr, stake) {
    const [marketId, runnerIdStr] = selectionIdStr.split(":");
    const runnerId = parseInt(runnerIdStr);

    console.log(`[FD] Fetching market prices: ${marketId}`);
    const prices = await this.getMarketPrices(marketId);
    const market = prices.markets?.[0];
    if (!market) throw new Error(`Market ${marketId} not found`);

    const runner = market.runners?.find((r) => r.selectionId === runnerId);
    if (!runner) throw new Error(`Runner ${runnerId} not found`);

    const decimalOdds = runner.winRunnerOdds?.trueOdds?.decimalOdds?.decimalOdds;
    const americanOdds = runner.winRunnerOdds?.americanDisplayOdds?.americanOdds;
    const fractional = runner.winRunnerOdds?.trueOdds?.fractionalOdds;

    console.log(`[FD] Selection ${runnerId} @ ${americanOdds} (${decimalOdds})`);

    console.log(`[FD] Validating bet (implyBets)...`);
    const implyResult = await this.implyBets(marketId, runnerId);

    if (implyResult.respCode !== "SUCCESS") {
      throw new Error(`implyBets failed: ${implyResult.respCode}`);
    }

    const combo = implyResult.betCombinations?.[0];
    if (!combo) throw new Error("No betCombination returned from implyBets");

    const betReference = combo.betReference;
    if (!betReference) throw new Error("No betReference in implyBets response");

    // Use raw trueOdds from implyBets (NOT display/pretty values)
    const implyOdds = combo.winAvgOdds || {};
    const trueDecimal = implyOdds.trueOdds?.decimalOdds?.decimalOdds || decimalOdds;
    const implyAmerican = implyOdds.americanDisplayOdds?.americanOdds;

    // Fractional odds: derive from american odds (e.g. +114 → 114/100, -122 → 100/122)
    let fracNum, fracDen;
    if (implyAmerican >= 0) {
      fracNum = Math.round(implyAmerican);
      fracDen = 100;
    } else {
      fracNum = 100;
      fracDen = Math.round(Math.abs(implyAmerican));
    }

    // Get the runner from implyBets (includes handicap when needed)
    const implyRunner = combo.legCombinations?.[0]?.runners?.[0] || {};

    console.log(`[FD] implyBets odds: ${implyAmerican} (${trueDecimal}), frac: ${fracNum}/${fracDen}, handicap: ${implyRunner.handicap ?? "none"}`);
    console.log(`[FD] Placing $${stake} bet...`);
    const placeResult = await this.placeBet({
      marketId: implyRunner.marketId || marketId,
      selectionId: implyRunner.selectionId || runnerId,
      handicap: implyRunner.handicap,
      stake,
      betReference,
      decimalOdds: trueDecimal,
      fractionalNumerator: fracNum,
      fractionalDenominator: fracDen,
    });

    if (placeResult.respCode !== "SUCCESS") {
      throw new Error(`placeBet failed: ${placeResult.respCode} — ${JSON.stringify(placeResult.betFailures || placeResult.errors || placeResult)}`);
    }

    const bet = placeResult.result?.[0];
    console.log(`[FD] Bet placed! ID: ${bet?.betReceiptId} | Status: ${bet?.resultCode}`);

    return {
      status: bet?.resultCode || placeResult.respCode,
      bets: [
        {
          type: "Single",
          stake,
          displayOdds: americanOdds,
          potentialReturns: bet?.totalPotentialWin,
          status: bet?.resultCode,
          betId: bet?.betId,
          receiptId: bet?.betReceiptId,
        },
      ],
    };
  }

  async saveCookies() {
    await this.browserManager.saveCookies(this.bookId);
  }
}

module.exports = { FanDuelAdapter, HOSTS };
