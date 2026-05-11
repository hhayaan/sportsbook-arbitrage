# 1. Open the bet-placer and engine in NEW tabs within the CURRENT window (-w 0)
$wtArgs = '-w 0 new-tab -d "C:\dev\sportsbook-arbitrage\bet-placer" powershell -NoExit -Command "npm start" ; new-tab -d "C:\dev\sportsbook-arbitrage\engine" powershell -NoExit'
Start-Process wt -ArgumentList $wtArgs

# 2. Change the directory of your CURRENT tab
Set-Location -Path "C:\dev\sportsbook-arbitrage"

# 3. Run the proxy in your CURRENT tab
node dk-proxy.js