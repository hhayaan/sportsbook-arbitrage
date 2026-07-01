const crypto = require("crypto");

const API_BASE = "https://api.arcadia.pinnacle.com/0.1";
const X_API_KEY = "CmX2KcMrXuFmNg6YFbmTxE0y9CIrOi0R";

class PinnacleAdapter {
  constructor(browserManager) {
    this.browserManager = browserManager;
    this.bookId = "pinnacle";
    this.deviceUuid = null;
    this.session = null;
    this.username = null;
    this._lastTokenRefresh = 0;
    this._tokenSource = null;
  }

  _getPage() {
    const page = this.browserManager.getPage(this.bookId);
    if (!page) throw new Error("Pinnacle browser not launched");
    return page;
  }

  async _refreshTokens() {
    const page = this._getPage();
    const user = await page.evaluate(`
      (() => {
        try {
          function fromObj(obj) {
            if (!obj || typeof obj !== "object") return null;
            const deviceUuid = obj.deviceId || obj.deviceUuid || obj.device_id || null;
            const session = obj.token || obj.session || obj.sessionId || obj.x_session || null;
            const username = obj.username || obj.userName || obj.login || null;
            if (deviceUuid && session) return { deviceUuid, session, username };
            return null;
          }

          const keys = ["Main:User", "User", "user", "auth", "Main:Auth", "session"];
          for (const key of keys) {
            const raw = localStorage.getItem(key);
            if (!raw) continue;
            try {
              const parsed = JSON.parse(raw);
              const top = fromObj(parsed);
              if (top) return { ...top, source: "localStorage:" + key };
              for (const [nestedKey, nestedValue] of Object.entries(parsed)) {
                const nested = fromObj(nestedValue);
                if (nested) return { ...nested, source: "localStorage:" + key + "." + nestedKey };
              }
            } catch (_) {}
          }

          for (let i = 0; i < localStorage.length; i++) {
            const key = localStorage.key(i);
            const raw = localStorage.getItem(key);
            if (!raw) continue;
            try {
              const parsed = JSON.parse(raw);
              const top = fromObj(parsed);
              if (top) return { ...top, source: "localStorage-scan:" + key };
            } catch (_) {}
          }

          for (const key of keys) {
            const raw = sessionStorage.getItem(key);
            if (!raw) continue;
            try {
              const parsed = JSON.parse(raw);
              const top = fromObj(parsed);
              if (top) return { ...top, source: "sessionStorage:" + key };
            } catch (_) {}
          }

          return { deviceUuid: null, session: null, username: null, source: null };
        } catch(e) { return { deviceUuid: null, session: null, username: null, source: null }; }
      })()
    `);

    this.deviceUuid = user.deviceUuid;
    this.session = user.session;
    this.username = user.username;
    if (user.source && user.source !== this._tokenSource) {
      console.log(`[PIN] Token source: ${user.source}`);
    }
    this._tokenSource = user.source;
    this._lastTokenRefresh = Date.now();
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

    if (!this.deviceUuid || !this.session) {
      throw new Error(
        'NOT_LOGGED_IN: Pinnacle browser is open but no usable session token was found. Log into https://www.pinnacle.ca in the opened Pinnacle window, then retry "balance pinnacle".'
      );
    }

    headers["x-device-uuid"] = this.deviceUuid;
    headers["x-session"] = this.session;

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

  async getLeagueSnapshot(leagueId) {
    const id = Number.parseInt(leagueId, 10);
    if (!Number.isInteger(id) || id <= 0) {
      throw new Error(`Invalid Pinnacle league id: ${leagueId}`);
    }

    const matchupsResp = await this._apiRequest(`${API_BASE}/leagues/${id}/matchups`);
    if (matchupsResp.status !== 200) {
      throw new Error(`matchups failed (${matchupsResp.status}): ${matchupsResp.body}`);
    }

    const marketsResp = await this._apiRequest(`${API_BASE}/leagues/${id}/markets/straight`);
    if (marketsResp.status !== 200) {
      throw new Error(`markets failed (${marketsResp.status}): ${marketsResp.body}`);
    }

    const matchups = matchupsResp.json();
    const markets = marketsResp.json();
    const relatedMatchups = [];
    const relatedMarkets = [];

    const includeRelated = !["0", "false", "off"].includes(
      String(process.env.PIN_SNAPSHOT_RELATED || "1").trim().toLowerCase()
    );
    const maxRelated = Number.parseInt(process.env.PIN_RELATED_MAX_MATCHUPS || "8", 10);
    const relatedLimit = Number.isFinite(maxRelated) && maxRelated > 0 ? maxRelated : 8;

    if (includeRelated) {
      const parentMatchups = matchups
        .filter((m) => m && m.type === "matchup" && m.id)
        .slice(0, relatedLimit);
      for (const matchup of parentMatchups) {
        try {
          const relatedResp = await this._apiRequest(`${API_BASE}/matchups/${matchup.id}/related`);
          if (relatedResp.status === 200) {
            const related = relatedResp.json();
            if (Array.isArray(related)) relatedMatchups.push(...related);
          }
        } catch (err) {
          console.debug(`[PIN] related matchups failed ${matchup.id}: ${err.message}`);
        }

        try {
          const relatedMarketsResp = await this._apiRequest(`${API_BASE}/matchups/${matchup.id}/markets/related/straight`);
          if (relatedMarketsResp.status === 200) {
            const related = relatedMarketsResp.json();
            if (Array.isArray(related)) relatedMarkets.push(...related);
          }
        } catch (err) {
          console.debug(`[PIN] related markets failed ${matchup.id}: ${err.message}`);
        }
      }
    }

    return {
      matchups,
      markets,
      related_matchups: relatedMatchups,
      related_markets: relatedMarkets,
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

    // marketKey grammar: s;{period};{m|ou|s}[;{line}] — for totals/spreads the line (e.g. 8.5)
    // is the 4th segment. Pinnacle requires `points` in the quote/place body for totals and
    // spreads (verified against captured accepted bets); moneyline (s;{p};m) has no line.
    const keySegments = marketKey.split(";");
    const marketType = keySegments[2]; // "m" | "ou" | "s"
    const rawLine = keySegments.length >= 4 ? parseFloat(keySegments[3]) : NaN;
    // Only treat the line as present when it parses to a finite number — a malformed marketKey
    // must NOT silently send `points:null` (JSON coerces NaN to null) on a money-placing request.
    const linePoints = Number.isFinite(rawLine) ? rawLine : undefined;

    // Step 1: Quote
    console.log(`[PIN] Quoting: matchup=${matchupId} market=${marketKey} side=${designation}`);
    const quoteResp = await this._apiRequest(`${API_BASE}/bets/straight/quote`, {
      method: "POST",
      body: {
        oddsFormat: "american",
        selections: [
          {
            matchupId,
            marketKey,
            designation,
            price: 0,
            ...(linePoints !== undefined ? { points: linePoints } : {}),
          },
        ],
      },
    });
    if (quoteResp.status !== 200) throw new Error(`quote failed (${quoteResp.status}): ${quoteResp.body}`);
    const quote = quoteResp.json();

    const sel = quote.selections?.[0];
    if (!sel) throw new Error("No selection in quote response");
    const marketId = sel.marketId;
    const price = sel.price;
    // Alt lines carry a marketAltId (main lines return null); the place body must echo both
    // `points` and `marketAltId` from the authoritative quote response for alt totals/spreads.
    const placePoints = sel.points ?? linePoints;
    const marketAltId = sel.marketAltId;

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
        acceptBetterPrices: true,
        acceptBetterPrice: true,
        class: "Straight",
        selections: [
          {
            marketId,
            matchupId,
            marketKey,
            designation,
            price,
            ...(placePoints !== undefined && placePoints !== null ? { points: placePoints } : {}),
            ...(marketAltId !== undefined && marketAltId !== null ? { marketAltId } : {}),
          },
        ],
        stake,
        originTag: marketType === "m" ? "sm:bsd" : "m:bsd",
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
