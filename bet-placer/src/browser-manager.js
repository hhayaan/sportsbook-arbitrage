const { chromium } = require("patchright");
const path = require("path");
const fs = require("fs");

const DATA_DIR = path.join(__dirname, "..", "..", "data");
const PROFILES_DIR = path.join(DATA_DIR, "profiles");
const COOKIES_DIR = path.join(DATA_DIR, "cookies");

class BrowserManager {
  constructor() {
    // bookId -> { browser, context, page }
    this.browsers = new Map();
  }

  /**
   * Launch a persistent Chrome window for a sportsbook.
   * Uses a separate Chromium user-data-dir per book (isolated sessions),
   * matching CheatKode's approach.
   */
  async launch(bookId, options = {}) {
    if (this.browsers.has(bookId)) {
      console.log(`[${bookId}] Browser already running`);
      return this.browsers.get(bookId);
    }

    const userDataDir = path.join(PROFILES_DIR, bookId);
    fs.mkdirSync(userDataDir, { recursive: true });

    console.log(`[${bookId}] Launching browser (profile: ${userDataDir})`);

    const context = await chromium.launchPersistentContext(userDataDir, {
      headless: false,
      channel: "chrome",
      viewport: null, // use full window size
      args: [
        "--disable-blink-features=AutomationControlled",
        "--start-maximized",
      ],
      ignoreDefaultArgs: ["--enable-automation"],
      ...options,
    });

    const pages = context.pages();
    const page = pages.length > 0 ? pages[0] : await context.newPage();

    const entry = { context, page, bookId };
    this.browsers.set(bookId, entry);

    context.on("close", () => {
      console.log(`[${bookId}] Browser closed`);
      this.browsers.delete(bookId);
    });

    return entry;
  }

  /**
   * Navigate a sportsbook browser to its login/home page.
   */
  async navigateTo(bookId, url) {
    const entry = this.browsers.get(bookId);
    if (!entry) throw new Error(`No browser for ${bookId}`);
    await entry.page.goto(url, { waitUntil: "domcontentloaded" });
    return entry.page;
  }

  /**
   * Save cookies/session data for a sportsbook.
   */
  async saveCookies(bookId) {
    const entry = this.browsers.get(bookId);
    if (!entry) return;

    const cookies = await entry.context.cookies();
    const filePath = path.join(COOKIES_DIR, `${bookId}-data.json`);
    fs.writeFileSync(filePath, JSON.stringify({ cookies }, null, 2));
    console.log(`[${bookId}] Cookies saved (${cookies.length} cookies)`);
  }

  /**
   * Load previously saved cookies into a browser context.
   */
  async loadCookies(bookId) {
    const filePath = path.join(COOKIES_DIR, `${bookId}-data.json`);
    if (!fs.existsSync(filePath)) return false;

    const entry = this.browsers.get(bookId);
    if (!entry) return false;

    const data = JSON.parse(fs.readFileSync(filePath, "utf-8"));
    if (data.cookies && data.cookies.length > 0) {
      await entry.context.addCookies(data.cookies);
      console.log(`[${bookId}] Loaded ${data.cookies.length} cookies`);
      return true;
    }
    return false;
  }

  /**
   * Get the page for a sportsbook (for automation).
   */
  getPage(bookId) {
    const entry = this.browsers.get(bookId);
    return entry ? entry.page : null;
  }

  /**
   * Close a specific sportsbook browser.
   */
  async close(bookId) {
    const entry = this.browsers.get(bookId);
    if (entry) {
      await entry.context.close();
      this.browsers.delete(bookId);
    }
  }

  /**
   * Close all browsers.
   */
  async closeAll() {
    for (const [bookId] of this.browsers) {
      await this.close(bookId);
    }
  }

  /**
   * Check if a sportsbook browser is running.
   */
  isRunning(bookId) {
    return this.browsers.has(bookId);
  }

  /**
   * List all running sportsbook browsers.
   */
  listRunning() {
    return Array.from(this.browsers.keys());
  }
}

module.exports = { BrowserManager };
