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
    token,
    calculationId,
    autoAcceptMode = "AcceptHigher",
  }) {
    const url = `${HOSTS.gaming}/api/wager/v1/placeBets`;
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
      selections: [{ id: selectionId, trueOdds, displayOdds }],
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

  /**
   * Save session cookies to disk.
   */
  async saveCookies() {
    await this.browserManager.saveCookies(this.bookId);
  }
}

module.exports = { DraftKingsAdapter, HOSTS };
