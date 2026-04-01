/**
 * Arb Placer — places both legs of an arbitrage bet simultaneously
 * across multiple sportsbook adapters.
 *
 * Each leg is: { bookId, selectionId, stake }
 * The adapters must already be initialized with active browser sessions.
 */

class ArbPlacer {
  constructor() {
    // bookId -> adapter instance
    this.adapters = new Map();
  }

  registerAdapter(bookId, adapter) {
    this.adapters.set(bookId, adapter);
  }

  getAdapter(bookId) {
    const adapter = this.adapters.get(bookId);
    if (!adapter) throw new Error(`No adapter registered for ${bookId}`);
    return adapter;
  }

  /**
   * Place an arbitrage bet across multiple books simultaneously.
   * @param {Array<{ bookId: string, selectionId: string, stake: number }>} legs
   * @returns {Promise<Array<{ bookId: string, success: boolean, result?: object, error?: string }>>}
   */
  async placeArb(legs) {
    console.log(`\n[ARB] Placing ${legs.length}-leg arb...`);

    // Fire all legs in parallel
    const results = await Promise.allSettled(
      legs.map(async (leg) => {
        const adapter = this.getAdapter(leg.bookId);
        console.log(`[ARB] Leg: ${leg.bookId} | ${leg.selectionId} | $${leg.stake}`);
        const result = await adapter.executeBet(leg.selectionId, leg.stake);
        return { bookId: leg.bookId, result };
      })
    );

    // Process results
    const summary = results.map((r, i) => {
      const leg = legs[i];
      if (r.status === "fulfilled") {
        return {
          bookId: leg.bookId,
          selectionId: leg.selectionId,
          stake: leg.stake,
          success: true,
          status: r.value.result.status,
          result: r.value.result,
        };
      } else {
        return {
          bookId: leg.bookId,
          selectionId: leg.selectionId,
          stake: leg.stake,
          success: false,
          error: r.reason?.message || String(r.reason),
        };
      }
    });

    // Log summary
    console.log(`\n[ARB] === Results ===`);
    for (const s of summary) {
      if (s.success) {
        const bet = s.result.bets?.[0];
        console.log(
          `  ${s.bookId}: OK — $${s.stake} @ ${bet?.displayOdds || "?"} → potential $${bet?.potentialReturns || "?"}`
        );
      } else {
        console.log(`  ${s.bookId}: FAILED — ${s.error}`);
      }
    }

    const allSuccess = summary.every((s) => s.success);
    const anySuccess = summary.some((s) => s.success);

    if (!allSuccess && anySuccess) {
      console.log(`\n[ARB] WARNING: Partial fill — some legs failed. May need recovery/hedge.`);
    } else if (!allSuccess) {
      console.log(`\n[ARB] All legs failed.`);
    } else {
      console.log(`\n[ARB] All legs placed successfully.`);
    }

    return summary;
  }
}

module.exports = { ArbPlacer };
