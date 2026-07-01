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

function fdTimeoutMs(name, fallback) {
  const raw = Number(process.env[name]);
  return Number.isFinite(raw) && raw >= 0 ? raw : fallback;
}

function typedTimeout(code, label, ms) {
  const err = new Error(`${code}: ${label || "operation"} timed out after ${ms || 0}ms`);
  err.code = code;
  err.label = label || null;
  err.timeoutMs = ms || 0;
  return err;
}

async function withTimeout(promise, ms, code, label) {
  if (!ms || ms <= 0) return promise;

  let timer = null;
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(() => reject(typedTimeout(code, label, ms)), ms);
  });

  try {
    return await Promise.race([promise, timeout]);
  } finally {
    if (timer) clearTimeout(timer);
  }
}

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

  _decodeJwtExpMs(jwt) {
    try {
      const parts = String(jwt || "").split(".");
      if (parts.length < 2) return 0;
      const payloadJson = Buffer.from(
        parts[1].replace(/-/g, "+").replace(/_/g, "/"),
        "base64"
      ).toString("utf8");
      const payload = JSON.parse(payloadJson);
      if (payload.expires) return new Date(payload.expires).getTime();
      if (payload.exp) return Number(payload.exp) * 1000;
    } catch (_) {}
    return 0;
  }

  _geoPacketFreshness(tokens, staleThresholdMs = 120_000) {
    const geo = tokens?.geoPacket;
    if (!geo) return { present: false, expMs: 0, secsLeft: 0, stale: true };

    const expMs = this._decodeJwtExpMs(geo);
    if (!expMs) return { present: true, expMs: 0, secsLeft: 0, stale: true };

    const secsLeft = Math.round((expMs - Date.now()) / 1000);
    return {
      present: true,
      expMs,
      secsLeft,
      stale: expMs - Date.now() < staleThresholdMs,
    };
  }

  async _refreshGeoPacket(reason) {
    const page = this._getPage();
    const startedAt = Date.now();
    console.log(`[FD geo] refresh triggered (${reason})`);

    try {
      await page.goto(HOSTS.origin + "/", { waitUntil: "domcontentloaded", timeout: 20_000 });

      for (let i = 0; i < 12; i += 1) {
        await page.waitForTimeout(500);
        const tokens = await this._getTokens();
        const freshness = this._geoPacketFreshness(tokens);
        if (freshness.present && !freshness.stale) {
          console.log(
            `[FD geo] refresh ok: jwt expires in ${freshness.secsLeft}s ` +
            `(elapsed=${Date.now() - startedAt}ms)`
          );
          return { tokens, freshness };
        }
      }
    } catch (err) {
      console.log(`[FD geo] refresh navigation error: ${err.message}`);
    }

    const tokens = await this._getTokens();
    const freshness = this._geoPacketFreshness(tokens);
    console.log(
      `[FD geo] refresh failed: ` +
      `${freshness.present ? `jwt ${freshness.secsLeft}s left` : "missing jwt"}`
    );
    return { tokens, freshness };
  }

  async _pxContextFromCookies() {
    const context = this._getContext();
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
    return pxContext;
  }

  _buildAppHeaders(tokens, pxContext, extraHeaders = {}) {
    const headers = {
      "content-type": "application/json",
      accept: "application/json",
      "x-application": X_APPLICATION,
      origin: HOSTS.origin,
      referer: HOSTS.origin + "/",
    };

    if (tokens?.authToken && typeof tokens.authToken === "string") {
      headers["x-authentication"] = tokens.authToken;
      headers["x-auth-token"] = tokens.authToken;
    }
    if (pxContext.length > 0) {
      headers["x-px-context"] = pxContext;
    }
    if (tokens?.geoPacket && typeof tokens.geoPacket === "string") {
      headers["x-geo-packet"] = tokens.geoPacket;
    }
    for (const [k, v] of Object.entries(extraHeaders)) {
      if (typeof v === "string") headers[k] = v;
    }

    return headers;
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

        // Active x-geo-packet source as of Hunter's 2026-04-18 FD audit:
        // localStorage.geolocation_document.jwt.
        // location_token.jwt is only a legacy fallback.
        let geoPacket = null;
        try {
          const activeDoc = JSON.parse(localStorage.getItem("geolocation_document") || "{}");
          geoPacket = activeDoc.jwt || null;
        } catch(e) {}
        try {
          if (!geoPacket) {
            const loc = JSON.parse(localStorage.getItem("location_token") || "{}");
            geoPacket = loc.jwt || null;
          }
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
    const pxContext = await this._pxContextFromCookies();
    const headers = this._buildAppHeaders(tokens, pxContext, extraHeaders);


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
   * POST from inside the logged-in FanDuel page. Hunter found that FD writes
   * can reject Node-side context.request calls with DSC-0015 when the
   * GeoComply x-geo-packet JWT is stale. Cashout uses this path so it refreshes
   * the geo packet only when needed, then writes with browser-page state.
   */
  async _pageApiPost(url, body, extraHeaders = {}, opts = {}) {
    const page = this._getPage();
    let tokens = await this._getTokens();
    const preFreshness = this._geoPacketFreshness(tokens);
    const writeTag = url.split("?")[0].split("/").slice(-2).join("/");

    console.log(
      `[FD write] ${writeTag} | geo: ` +
      `${preFreshness.present ? `jwt ${preFreshness.secsLeft}s left` : "missing"}`
    );

    if (preFreshness.stale) {
      const { tokens: freshTokens, freshness } = await this._refreshGeoPacket(
        preFreshness.present ? "stale_jwt" : "missing_jwt"
      );
      if (freshTokens) tokens = freshTokens;
      if (freshness.stale) {
        console.log("[FD write] proceeding with stale geo JWT after refresh miss");
      }
    }

    const pxContext = await this._pxContextFromCookies();
    const headers = this._buildAppHeaders(tokens, pxContext, extraHeaders);
    const bodyStr = typeof body === "string" ? body : JSON.stringify(body);
    const timeoutMs = Number.isFinite(Number(opts.timeoutMs)) ? Number(opts.timeoutMs) : 0;
    const timeoutCode = opts.code || "FD_PAGE_API_TIMEOUT";
    const timeoutLabel = opts.label || `fd_${writeTag}`;

    const evalPromise = page.evaluate(async (cfg) => {
      const controller = cfg.timeoutMs > 0 ? new AbortController() : null;
      const timer = controller ? setTimeout(() => controller.abort(), cfg.timeoutMs) : null;
      try {
        const resp = await fetch(cfg.url, {
          method: "POST",
          headers: cfg.headers,
          credentials: "include",
          body: cfg.body,
          signal: controller ? controller.signal : undefined,
        });
        const responseHeaders = {};
        resp.headers.forEach((value, key) => {
          responseHeaders[key] = value;
        });
        return {
          status: resp.status,
          headers: responseHeaders,
          body: await resp.text(),
        };
      } catch (e) {
        return {
          status: 0,
          headers: {},
          body: e && e.message ? e.message : String(e),
          aborted: !!(e && e.name === "AbortError"),
        };
      } finally {
        if (timer) clearTimeout(timer);
      }
    }, {
      url,
      headers,
      body: bodyStr,
      timeoutMs,
    });

    const result = await withTimeout(
      evalPromise,
      timeoutMs > 0 ? timeoutMs + 500 : 0,
      timeoutCode,
      timeoutLabel
    );
    if (result && result.aborted) {
      throw typedTimeout(timeoutCode, timeoutLabel, timeoutMs);
    }

    return {
      status: result.status,
      headers: result.headers || {},
      body: result.body || "",
      json: () => JSON.parse(result.body || "{}"),
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
    const implyResult = await this.implyBets(marketId, runnerId, runner.handicap);

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

    // Get the runner from implyBets for marketId/selectionId echo.
    const implyRunner = combo.legCombinations?.[0]?.runners?.[0] || {};

    // Source the handicap from the getMarketPrices runner (line ~547), NOT from implyBets:
    // betCombinations[].legCombinations[].runners[] never carries `handicap`, so
    // implyRunner.handicap is always undefined and main run lines / totals / pitcher O/U
    // would otherwise place at handicap 0 (wrong line or FD rejection). Alt lines and
    // discrete "N+" props legitimately have handicap 0 (line is baked into the selectionId).
    const handicap = runner.handicap;

    console.log(`[FD] implyBets odds: ${implyAmerican} (${trueDecimal}), frac: ${fracNum}/${fracDen}, handicap: ${handicap ?? "none"}`);
    console.log(`[FD] Placing $${stake} bet...`);
    const placeResult = await this.placeBet({
      marketId: implyRunner.marketId || marketId,
      selectionId: implyRunner.selectionId || runnerId,
      handicap,
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
      _betId: bet?.betId || null,
      _purchaseId: bet?.betId || null,
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

  /**
   * Fetch open bets with fresh cash-out tokens from FD.
   * Hunter HAR source: GET fcq.on.sportsbook.fanduel.ca/api/v0.1/getEligibleBets.
   */
  async getOpenBets() {
    const url = "https://fcq.on.sportsbook.fanduel.ca/api/v0.1/getEligibleBets?sortBy=START_TIME";
    try {
      const resp = await this._apiRequest(url);
      if (resp.status !== 200) {
        throw new Error(`getEligibleBets failed (${resp.status}): ${resp.body}`);
      }

      const data = resp.json();
      const sections = [
        ...(data.inplayBets?.bets || []),
        ...(data.comingUpBets?.bets || []),
      ];

      const bets = sections.map((b) => {
        const quoteResult = b.quoteResult || {};
        const leg = b.legs?.[0] || {};
        const cashoutAvailable =
          quoteResult.quoteStatus === "AVAILABLE" && !!quoteResult.cashOutToken;
        return {
          betId: b.betId,
          receiptId: b.betReceiptId,
          status: b.status,
          stake: b.currentSize || b.size,
          potentialReturns: b.potentialWin,
          description:
            `${leg.selectionName || "?"} | ` +
            `${leg.eventDescription || ""} | ` +
            `${leg.eventMarketDescription || ""}`,
          cashoutOffer: quoteResult.quote || null,
          cashoutToken: quoteResult.cashOutToken || null,
          cashoutAvailable,
          betDelay: quoteResult.betDelay || 0,
          raw: b,
        };
      });

      console.log(`[FD] getOpenBets: ${bets.length} open bet(s)`);
      for (const bet of bets) {
        console.log(
          `  ${bet.betId} | $${bet.stake} | ` +
          `cashout: ${bet.cashoutAvailable ? "YES" : "N/A"} | ${bet.description}`
        );
      }
      return bets;
    } catch (err) {
      console.error(`[FD] getOpenBets failed: ${err.message}`);
      return [];
    }
  }

  async getBetStatus(_purchaseId, betId) {
    const id = betId || _purchaseId;
    if (!id) {
      return {
        status: "unknown",
        purchase_id: null,
        bet_id: null,
        raw_status: null,
        cashout_available: false,
        cashout_offer: null,
        error: "FD getBetStatus requires bet_id",
      };
    }

    const bets = await this.getOpenBets();
    const match = bets.find((b) => String(b.betId) === String(id));
    if (!match) {
      return {
        status: "unknown",
        purchase_id: String(id),
        bet_id: String(id),
        raw_status: "Not Found In Open Bets",
        cashout_available: false,
        cashout_offer: null,
      };
    }

    return {
      status: "open",
      purchase_id: String(match.betId),
      bet_id: String(match.betId),
      raw_status: match.status || "Open",
      cashout_available: match.cashoutAvailable,
      cashout_offer: match.cashoutOffer,
      raw: match.raw,
    };
  }

  /**
   * Cash out a specific FD bet. A fresh cashOutToken is fetched immediately
   * before the write; the token is not interchangeable with the bet id.
   */
  async cashOut(betId) {
    if (!betId) throw new Error("FD cashOut requires betId");
    console.log(`[FD] Cashing out betId=${betId}...`);

    const bets = await this.getOpenBets();
    const match = bets.find((b) => String(b.betId) === String(betId));
    if (!match) throw new Error(`Bet ${betId} not found in open bets`);
    if (!match.cashoutToken) {
      throw new Error(`Bet ${betId} has no cashOutToken (not eligible for cash-out)`);
    }

    const url = "https://sco.on.sportsbook.fanduel.ca/api/sports/fixedodds/transactional/v1.0/cashoutBet";
    const body = {
      quote: match.cashoutOffer || 1,
      betDelay: match.betDelay || 0,
      cashOutToken: match.cashoutToken,
      betId: String(betId),
    };

    const resp = await this._pageApiPost(url, body, {}, {
      timeoutMs: fdTimeoutMs("FD_WRITE_TIMEOUT_MS", 25_000),
      code: "FD_WRITE_STATUS_UNKNOWN",
      label: "fd_cashoutBet",
    });
    if (resp.status !== 200) {
      throw new Error(`FD cashoutBet failed (${resp.status}): ${resp.body}`);
    }

    const result = resp.json();
    console.log(
      `[FD] Cash-out result: respStatus=${result.respStatus} ` +
      `cashedOutQuote=${result.cashedOutQuote}`
    );

    if (result.respStatus !== "SUCCESS") {
      throw new Error(`FD cashout rejected: ${result.respStatus}`);
    }

    return {
      success: true,
      confirmed: true,
      payout: Number(result.cashedOutQuote ?? match.cashoutOffer ?? 0),
      raw_status: "Cashed Out",
      raw: result,
    };
  }

  async saveCookies() {
    await this.browserManager.saveCookies(this.bookId);
  }
}

module.exports = { FanDuelAdapter, HOSTS };
