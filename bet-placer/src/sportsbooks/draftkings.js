const fs = require("fs");
const path = require("path");

const COOKIES_DIR = path.join(__dirname, "..", "..", "..", "data", "cookies");

// DraftKings API hosts (CA-ON region — adjust for your region)
const HOSTS = {
  gaming: "https://gaming-ca-on.draftkings.com",
  content: "https://sportsbook-nash.draftkings.com",
  api: "https://api.draftkings.com",
  origin: "https://sportsbook.draftkings.com",
};

// Standard headers sent with every wager API call
const WAGER_HEADERS = {
  clienttype: "Website",
  "content-type": "application/json",
  "x-api-features": '{"EnableFullSGPDrivenFlow":true}',
  "x-client-feature": "betslip",
  "x-client-name": "web",
  "x-client-version": "2612.4.1.6",
  "x-client-widget-name": "betslip",
  "x-client-widget-version": "2608.3.1",
};

const DK_RAW_STATUS_OPEN_RE =
  /\bunsettled\b|\bnot\s+settled\b|\bopen\b|\bpending\b|\bplaced\b|\baccepted\b|\bprocessing\b|\blive\s*delay\b|\blivedelay\b|\bactive\b/i;
const DK_RAW_STATUS_LIVE_PENDING_RE =
  /\bpending\b|\bprocessing\b|\blive\s*delay\b|\blivedelay\b/i;
const DK_RAW_STATUS_CASHED_OUT_RE =
  /\bcashed\s*out\b|\bcashedout\b|\bcash\s*out\b/i;
const DK_RAW_STATUS_VOIDED_RE =
  /\bvoid(?:ed)?\b|\bcancell?ed\b|\bcancelled\b|\bcancel\b|\brefund(?:ed)?\b|\bpush\b/i;
const DK_RAW_STATUS_SETTLED_RE =
  /\bsettled\b|\bclosed\b|\bcomplete(?:d)?\b|\bresulted\b|\bpaid\b|\bgraded\b|\bwon\b|\bwin\b|\bwinner\b|\blost\b|\bloss\b|\bloser\b/i;

function classifyDraftKingsRawStatus(rawStatus) {
  const raw = String(rawStatus ?? "").trim();
  if (!raw) return "unknown";
  if (DK_RAW_STATUS_OPEN_RE.test(raw)) {
    return DK_RAW_STATUS_LIVE_PENDING_RE.test(raw) ? "live_pending" : "open";
  }
  if (DK_RAW_STATUS_CASHED_OUT_RE.test(raw)) return "cashed_out";
  if (DK_RAW_STATUS_VOIDED_RE.test(raw)) return "voided";
  if (DK_RAW_STATUS_SETTLED_RE.test(raw)) return "settled";
  return "unknown";
}

function parseMoneyAmount(value) {
  if (value === null || value === undefined || value === "") return null;
  if (typeof value === "number" && Number.isFinite(value)) return value;
  const n = Number(String(value).replace(/[^0-9.-]/g, ""));
  return Number.isFinite(n) ? n : null;
}

function safeBetIdFromPurchaseId(purchaseId) {
  const n = Number(purchaseId);
  return Number.isSafeInteger(n) ? String(n + 1) : null;
}

class DraftKingsAdapter {
  /**
   * @param {import('../browser-manager').BrowserManager} browserManager
   */
  constructor(browserManager) {
    this.browserManager = browserManager;
    this.bookId = "draftkings";
  }

  /**
   * Get the browser page for DraftKings.
   */
  _getPage() {
    const page = this.browserManager.getPage(this.bookId);
    if (!page) throw new Error("DraftKings browser not launched");
    return page;
  }

  /**
   * Make an API request using the browser's authenticated context.
   * This sends cookies automatically (just like the real site does).
   */
  async _apiRequest(url, options = {}) {
    const page = this._getPage();
    const { method = "GET", body = null, extraHeaders = {} } = options;

    const headers = {
      ...WAGER_HEADERS,
      ...extraHeaders,
    };

    const fetchScript = `
      (async () => {
        const resp = await fetch(${JSON.stringify(url)}, {
          method: ${JSON.stringify(method)},
          headers: ${JSON.stringify(headers)},
          credentials: "include",
          ${body ? `body: ${JSON.stringify(JSON.stringify(body))},` : ""}
        });
        const responseHeaders = {};
        resp.headers.forEach((v, k) => responseHeaders[k] = v);
        const text = await resp.text();
        return { status: resp.status, headers: responseHeaders, body: text };
      })()
    `;

    const result = await page.evaluate(fetchScript);
    result.json = () => JSON.parse(result.body);
    return result;
  }

  /**
   * Step 1: Fetch selection info (odds, event details).
   * @param {string} selectionId e.g. "0ML84034728_3"
   */
  async getSelectionInfo(selectionId) {
    const url =
      `${HOSTS.content}/api/sdinfo/dkcaon/v1/selectioninfo` +
      `?siteName=dkcaon&selectionIds=${encodeURIComponent(selectionId)}`;

    const resp = await this._apiRequest(url);
    if (resp.status !== 200) {
      throw new Error(`getSelectionInfo failed (${resp.status}): ${resp.body}`);
    }
    return resp.json();
  }

  /**
   * Step 2: Calculate bets — validates the selection and returns a
   * server-signed token required for placement.
   * @param {string[]} selectionIds
   * @returns {{ data: object, calculationId: string }}
   */
  async calculateBets(selectionIds) {
    const url = `${HOSTS.gaming}/api/wager/v1/calculateBets`;
    const body = {
      selections: selectionIds.map((id) => ({ id })),
      selectionsForYourBet: [],
      oddsStyle: "american",
    };

    const resp = await this._apiRequest(url, { method: "POST", body });
    if (resp.status !== 200) {
      throw new Error(`calculateBets failed (${resp.status}): ${resp.body}`);
    }

    return {
      data: resp.json(),
      calculationId: resp.headers["calculationid"] || resp.headers["CalculationId"],
    };
  }

  /**
   * Step 3: Place the bet.
   * @param {object} params
   * @param {string} params.selectionId
   * @param {number} params.stake
   * @param {number} params.trueOdds
   * @param {string} params.displayOdds e.g. "+350"
   * @param {string} params.token from calculateBets
   * @param {string} params.calculationId from calculateBets response header
   * @param {string} [params.autoAcceptMode="AcceptHigher"]
   */
  async placeBet({
    selectionId,
    stake,
    trueOdds,
    displayOdds,
    points,
    token,
    calculationId,
    autoAcceptMode = "AcceptHigher",
  }) {
    const url = `${HOSTS.gaming}/api/wager/v1/placeBets`;
    const selectionObj = { id: selectionId, trueOdds, displayOdds };
    if (points !== undefined) selectionObj.points = points;
    const body = {
      bets: [
        {
          type: "Single",
          selectionsMapped: [{ id: selectionId }],
          stake,
          trueOdds,
          displayOdds,
          numberOfBets: 1,
          isSP: false,
          potentialReturns: Math.floor(stake * trueOdds * 100) / 100,
          token,
        },
      ],
      selections: [selectionObj],
      selectionsForYourBet: [],
      locale: "en-US",
      oddsStyle: "american",
      autoAcceptMode,
    };

    const resp = await this._apiRequest(url, {
      method: "POST",
      body,
      extraHeaders: {
        calculationid: calculationId,
        "x-request-client-timestamp": Date.now().toString(),
      },
    });

    if (resp.status !== 200) {
      throw new Error(`placeBets failed (${resp.status}): ${resp.body}`);
    }
    return resp.json();
  }

  /**
   * Step 4: Poll for bet confirmation.
   * @param {string} waitingBetId from placeBet response
   * @param {number} [maxWaitMs=15000]
   * @param {number} [pollIntervalMs=1000]
   */
  async waitForConfirmation(waitingBetId, maxWaitMs = 15000, pollIntervalMs = 1000) {
    const url =
      `${HOSTS.gaming}/api/wager/v1/getPurchases` +
      `?PurchaseIds=${encodeURIComponent(waitingBetId)}&format=json`;

    const start = Date.now();
    while (Date.now() - start < maxWaitMs) {
      const resp = await this._apiRequest(url);
      if (resp.status === 200) {
        const data = resp.json();
        const purchase = data.purchases?.[0];
        if (purchase && purchase.status !== "LiveDelay") {
          return purchase;
        }
      }
      await new Promise((r) => setTimeout(r, pollIntervalMs));
    }
    throw new Error(`Bet confirmation timed out after ${maxWaitMs}ms`);
  }

  /**
   * Get account balance.
   */
  async getBalance() {
    const url = `${HOSTS.api}/sites/CA-ON-SB/dashes/v1/dashes/siteNav/users/me.json?format=json`;
    const resp = await this._apiRequest(url);
    if (resp.status !== 200) {
      throw new Error(`getBalance failed (${resp.status}): ${resp.body}`);
    }
    const data = resp.json();
    const cashAccounts = (data.accounts || []).filter(a => a.currencyCode !== "XPP");
    return {
      accounts: cashAccounts,
      username: data.userName,
    };
  }

  /**
   * Full bet placement flow: calculate → place → confirm.
   * @param {string} selectionId
   * @param {number} stake
   */
  async executeBet(selectionId, stake) {
    console.log(`[DK] Fetching selection info: ${selectionId}`);
    const info = await this.getSelectionInfo(selectionId);
    const selection = info.selections?.[0];
    if (!selection) throw new Error(`Selection ${selectionId} not found`);

    console.log(
      `[DK] Selection: ${selection.betSlipLine} @ ${selection.displayOdds.american} (${selection.trueOdds})`
    );

    console.log(`[DK] Calculating bet...`);
    const { data: calcData, calculationId } = await this.calculateBets([selectionId]);
    const bet = calcData.bets?.[0];
    if (!bet) throw new Error("No bet returned from calculateBets");

    const currentOdds = calcData.selections[0];
    console.log(`[DK] Odds: ${currentOdds.displayOdds} (${currentOdds.trueOdds})`);

    console.log(`[DK] Placing $${stake} bet...`);
    const placeResult = await this.placeBet({
      selectionId,
      stake,
      trueOdds: currentOdds.trueOdds,
      displayOdds: currentOdds.displayOdds,
      points: currentOdds.points,
      token: bet.token,
      calculationId,
    });

    if (placeResult.waitingBetId) {
      console.log(
        `[DK] Bet submitted (status: ${placeResult.status}), waiting for confirmation...`
      );
      const confirmation = await this.waitForConfirmation(placeResult.waitingBetId);
      console.log(`[DK] Bet confirmed! Status: ${confirmation.status}`);
      return confirmation;
    }

    console.log(`[DK] Bet result: ${placeResult.status}`);
    return placeResult;
  }

  _extractCashoutOffer(purchase) {
    return parseMoneyAmount(
      purchase?.cashOutValue ??
      purchase?.cashoutValue ??
      purchase?.cashOutAmount
    );
  }

  async _getBetStatusByPurchaseId(purchaseId) {
    const url =
      `${HOSTS.gaming}/api/wager/v1/getPurchases` +
      `?PurchaseIds=${encodeURIComponent(purchaseId)}&format=json`;
    const resp = await this._apiRequest(url);
    if (resp.status !== 200) {
      return {
        status: "unknown",
        purchase_id: String(purchaseId),
        bet_id: null,
        raw_status: null,
        found: false,
        source: "getPurchases",
        error: `getPurchases failed (${resp.status})`,
      };
    }

    const data = resp.json();
    const purchase = data?.purchases?.[0] || data?.purchase || (Array.isArray(data) ? data[0] : null);
    if (!purchase) {
      return {
        status: "unknown",
        purchase_id: String(purchaseId),
        bet_id: null,
        raw_status: null,
        found: false,
        source: "getPurchases",
        error: "purchase_not_found",
      };
    }

    const rawStatus = purchase.status || purchase.purchaseStatus || purchase.betStatus || null;
    const status = classifyDraftKingsRawStatus(rawStatus);
    const cashoutOffer = this._extractCashoutOffer(purchase);
    return {
      status,
      purchase_id: String(purchase.purchaseId || purchase.id || purchaseId),
      bet_id: String(purchase.betId || purchase.bets?.[0]?.betId || safeBetIdFromPurchaseId(purchaseId) || ""),
      raw_status: rawStatus,
      found: true,
      source: "getPurchases",
      cashout_available: cashoutOffer !== null && cashoutOffer > 0,
      cashout_offer: cashoutOffer,
    };
  }

  async getBetStatus(purchaseId, betId = null) {
    const ids = [...new Set([purchaseId, betId].filter(Boolean).map((id) => String(id)))];
    if (ids.length === 0) {
      return {
        status: "unknown",
        purchase_id: null,
        bet_id: null,
        raw_status: null,
        found: false,
        source: "getPurchases",
        error: "missing_purchase_id",
      };
    }

    let last = null;
    for (const id of ids) {
      last = await this._getBetStatusByPurchaseId(id);
      if (last.found || last.status !== "unknown") return last;
    }
    return last;
  }

  async cashOut(purchaseId, cashoutValue = null, betId = null) {
    if (!purchaseId) throw new Error("cashOut requires purchaseId");

    if (!cashoutValue) {
      const status = await this._getBetStatusByPurchaseId(purchaseId);
      cashoutValue = status.cashout_offer;
      betId = betId || status.bet_id;
      if (!cashoutValue) {
        throw new Error(`No cashout offer available for ${purchaseId}`);
      }
    }

    if (!betId) betId = safeBetIdFromPurchaseId(purchaseId) || String(purchaseId);

    const url = `${HOSTS.gaming}/api/wager/v1/cashout`;
    const body = {
      cashOutInformation: {
        betId: String(betId),
        purchaseId: String(purchaseId),
        cashOutEnabled: true,
        partialCashOutEnabled: false,
        fullReturn: cashoutValue,
        cashOutStatus: "Confirm",
      },
      cashOutAmount: cashoutValue,
    };

    const resp = await this._apiRequest(url, { method: "POST", body });
    if (resp.status !== 200) {
      throw new Error(`cashout failed (${resp.status}): ${resp.body}`);
    }

    const data = resp.json();
    const responseRawStatus = data?.status || null;
    const responseStatusClass = classifyDraftKingsRawStatus(responseRawStatus);
    if (responseStatusClass === "cashed_out") {
      return {
        success: true,
        confirmed: true,
        payout: cashoutValue,
        raw_status: responseRawStatus,
        raw: data,
      };
    }
    if (responseStatusClass === "settled" || responseStatusClass === "voided") {
      return {
        success: false,
        submitted: true,
        terminal_non_cashout: true,
        reason: "DK_CASHOUT_TERMINAL_NON_CASHOUT",
        payout_claimed: cashoutValue,
        raw_status: responseRawStatus,
        raw: data,
      };
    }

    if (data.status === "LiveDelay") {
      const delaySeconds = Number.isFinite(Number(data.delayPeriod)) ? Math.max(0, Number(data.delayPeriod)) : 4;
      await new Promise((resolve) => setTimeout(resolve, delaySeconds * 1000 + 1000));
      const status = await this._getBetStatusByPurchaseId(purchaseId);
      const statusClass = classifyDraftKingsRawStatus(status.raw_status);
      if (statusClass === "cashed_out") {
        return {
          success: true,
          confirmed: true,
          payout: cashoutValue,
          raw_status: status.raw_status,
          raw: status,
        };
      }
      if (statusClass === "settled" || statusClass === "voided") {
        return {
          success: false,
          submitted: true,
          terminal_non_cashout: true,
          reason: "DK_CASHOUT_TERMINAL_NON_CASHOUT",
          payout_claimed: cashoutValue,
          raw_status: status.raw_status,
          raw: status,
        };
      }
      return {
        success: false,
        unconfirmed: true,
        submitted: true,
        reason: "DK_CASHOUT_NOT_CONFIRMED",
        payout_claimed: cashoutValue,
        raw_status: status.raw_status,
        raw: status,
      };
    }

    return {
      success: false,
      unconfirmed: true,
      submitted: true,
      reason: "DK_CASHOUT_NOT_CONFIRMED",
      payout_claimed: cashoutValue,
      raw_status: responseRawStatus,
      raw: data,
    };
  }

  /**
   * Save session cookies to disk.
   */
  async saveCookies() {
    await this.browserManager.saveCookies(this.bookId);
  }
}

module.exports = { DraftKingsAdapter, HOSTS };
