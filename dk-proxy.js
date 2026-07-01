// DK Proxy - works around Rust reqwest TLS incompatibility with DK's CDN on Windows.
// Uses Node's fetch API which handles TLS correctly.
// Listens on port 3403.

const http = require("http");

const PORT = parseInt(process.env.DK_PROXY_PORT || "3403", 10);
const configuredFetchTimeoutMs = parseInt(
  process.env.DK_PROXY_FETCH_TIMEOUT_MS || process.env.ODDS_FETCH_TIMEOUT_MS || "2500",
  10
);
const DK_FETCH_TIMEOUT_MS = Number.isFinite(configuredFetchTimeoutMs)
  && configuredFetchTimeoutMs >= 250
  && configuredFetchTimeoutMs <= 30000
  ? configuredFetchTimeoutMs
  : 2500;
const DK_HOST = "sportsbook-nash.draftkings.com";

function buildDraftKingsUrl(reqUrl) {
  const leagueMatch = reqUrl.match(/^\/dk\/(\d+)/);
  if (leagueMatch) {
    return `https://${DK_HOST}/sites/CA-ON-SB/api/sportscontent/dkcaon/v1/leagues/${leagueMatch[1]}`;
  }

  const eventSubcategoryMatch = reqUrl.match(/^\/dk-event\/(\d+)\/(\d+)/);
  if (eventSubcategoryMatch) {
    const eventId = eventSubcategoryMatch[1];
    const subcategoryId = eventSubcategoryMatch[2];
    const marketsQuery = `$filter=eventId eq '${eventId}' AND clientMetadata/subCategoryId eq '${subcategoryId}' AND tags/all(t: t ne 'SportcastBetBuilder')`;
    const params = new URLSearchParams({
      isBatchable: "false",
      templateVars: `${eventId},${subcategoryId}`,
      marketsQuery,
      entity: "markets",
    });
    return `https://${DK_HOST}/sites/CA-ON-SB/api/sportscontent/controldata/event/eventSubcategory/v1/markets?${params}`;
  }

  return null;
}

const server = http.createServer(async (req, res) => {
  const dkUrl = buildDraftKingsUrl(req.url);
  if (!dkUrl) {
    res.writeHead(404);
    res.end("Not found. Use /dk/{leagueId} or /dk-event/{eventId}/{subcategoryId}");
    return;
  }

  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), DK_FETCH_TIMEOUT_MS);

  try {
    const resp = await fetch(dkUrl, {
      signal: controller.signal,
      headers: {
        "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
        "Accept": "application/json",
      },
    });

    const body = await resp.text();
    clearTimeout(timeout);
    res.writeHead(resp.status, {
      "Content-Type": "application/json",
      "Access-Control-Allow-Origin": "*",
    });
    res.end(body);
    console.log(`[DK Proxy] ${req.url} -> ${resp.status} (${body.length} bytes)`);
  } catch (err) {
    clearTimeout(timeout);
    const message = err.name === "AbortError"
      ? `timeout after ${DK_FETCH_TIMEOUT_MS}ms`
      : err.message;
    console.error(`[DK Proxy] Error fetching ${req.url}: ${message}`);
    res.writeHead(502);
    res.end(JSON.stringify({ error: message }));
  }
});

server.listen(PORT, () => {
  console.log(`[DK Proxy] Listening on http://localhost:${PORT}`);
  console.log(`[DK Proxy] Route: /dk/{leagueId} -> ${DK_HOST}`);
  console.log(`[DK Proxy] Route: /dk-event/{eventId}/{subcategoryId} -> ${DK_HOST}`);
});

process.on("SIGINT", () => {
  server.close();
  process.exit(0);
});

server.on("error", (err) => {
  if (err.code === "EADDRINUSE") {
    console.error(`[DK Proxy] Port ${PORT} already in use`);
  } else {
    console.error(`[DK Proxy] Server error: ${err.message}`);
  }
  process.exit(1);
});
