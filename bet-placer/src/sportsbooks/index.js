/**
 * Sportsbook registry — URLs and configuration for each supported book.
 * Matches CheatKode's supported sportsbooks.
 */

const SPORTSBOOKS = {
  draftkings: {
    name: "DraftKings",
    url: "https://sportsbook.draftkings.com",
    loginUrl: "https://sportsbook.draftkings.com",
  },
  fanduel: {
    name: "FanDuel",
    url: "https://on.sportsbook.fanduel.ca",
    loginUrl: "https://on.sportsbook.fanduel.ca",
  },
  pinnacle: {
    name: "Pinnacle",
    url: "https://www.pinnacle.ca",
    loginUrl: "https://www.pinnacle.ca",
  },
  espnbet: {
    name: "ESPN BET",
    url: "https://sportsbook.thescore.bet",
    loginUrl: "https://sportsbook.thescore.bet",
  },
  betmgm: {
    name: "BetMGM",
    url: "https://sports.betmgm.com",
    loginUrl: "https://sports.betmgm.com",
  },
  pointsbet: {
    name: "PointsBet",
    url: "https://pointsbet.com",
    loginUrl: "https://pointsbet.com",
  },
  betano: {
    name: "Betano",
    url: "https://www.betano.com",
    loginUrl: "https://www.betano.com",
  },
};

module.exports = { SPORTSBOOKS };
