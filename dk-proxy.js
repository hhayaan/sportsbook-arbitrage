// DK Proxy — works around Rust reqwest TLS incompatibility with DK's CDN on Windows.
// Uses Node's fetch API which handles TLS correctly.
// Listens on port 3403.

const http = require("http");

const PORT = parseInt(process.env.DK_PROXY_PORT || "3403", 10);
const DK_HOST = "sportsbook-nash.draftkings.com";

const server = http.createServer(async (req, res) => {
  const match = req.url.match(/^\/dk\/(\d+)/);
  if (!match) {
    res.writeHead(404);
    res.end("Not found. Use /dk/{leagueId}");
    return;
  }

  const leagueId = match[1];
  const dkUrl = `https://${DK_HOST}/sites/CA-ON-SB/api/sportscontent/dkcaon/v1/leagues/${leagueId}`;

  try {
    const resp = await fetch(dkUrl, {
      headers: {
        "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
        "Accept": "application/json",
      },
    });

    const body = await resp.text();
    res.writeHead(resp.status, {
      "Content-Type": "application/json",
      "Access-Control-Allow-Origin": "*",
    });
    res.end(body);
    console.log(`[DK Proxy] /dk/${leagueId} → ${resp.status} (${body.length} bytes)`);
  } catch (err) {
    console.error(`[DK Proxy] Error fetching league ${leagueId}: ${err.message}`);
    res.writeHead(502);
    res.end(JSON.stringify({ error: err.message }));
  }
});

server.listen(PORT, () => {
  console.log(`[DK Proxy] Listening on http://localhost:${PORT}`);
  console.log(`[DK Proxy] Route: /dk/{leagueId} -> ${DK_HOST}`);
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
