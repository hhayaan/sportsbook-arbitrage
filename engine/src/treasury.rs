use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Clone)]
pub struct VenueBalance {
    pub venue: String,
    pub free: f64,
    pub reserved: f64,
    pub in_flight: f64,
    pub initial: f64,
}

impl VenueBalance {
    pub fn total(&self) -> f64 {
        self.free + self.reserved + self.in_flight
    }
}

pub struct Treasury {
    balances: Mutex<HashMap<String, VenueBalance>>,
}

impl Treasury {
    pub fn new() -> Self {
        Treasury {
            balances: Mutex::new(HashMap::new()),
        }
    }

    pub fn initialize_venue(&self, venue: &str, initial_balance: f64) {
        let mut balances = self.balances.lock().unwrap();
        balances.insert(
            venue.to_string(),
            VenueBalance {
                venue: venue.to_string(),
                free: initial_balance,
                reserved: 0.0,
                in_flight: 0.0,
                initial: initial_balance,
            },
        );
        tracing::info!(venue = %venue, balance = initial_balance, "Treasury: venue initialized");
    }

    pub fn initialize_venue_with_balance(&self, venue: &str, free: f64, reserved: f64, in_flight: f64, initial: f64) {
        let mut balances = self.balances.lock().unwrap();
        balances.insert(
            venue.to_string(),
            VenueBalance { venue: venue.to_string(), free, reserved, in_flight, initial },
        );
        tracing::info!(venue = %venue, free, reserved, in_flight, initial, "Treasury: restored");
    }

    /// Reserve capital: free → reserved
    pub fn reserve(&self, venue: &str, amount: f64) -> Result<(), String> {
        let mut balances = self.balances.lock().unwrap();
        let b = balances.get_mut(venue).ok_or_else(|| format!("Unknown venue: {}", venue))?;
        if b.free < amount {
            return Err(format!("{}: free=${:.2} < required=${:.2}", venue, b.free, amount));
        }
        b.free -= amount;
        b.reserved += amount;
        tracing::debug!(venue = %venue, amount, free = b.free, reserved = b.reserved, "Treasury: reserve");
        Ok(())
    }

    /// Commit to execution: reserved → in_flight
    pub fn commit(&self, venue: &str, amount: f64) -> Result<(), String> {
        let mut balances = self.balances.lock().unwrap();
        let b = balances.get_mut(venue).ok_or_else(|| format!("Unknown venue: {}", venue))?;
        if b.reserved < amount - 0.01 {
            return Err(format!("{}: reserved=${:.2} < commit=${:.2}", venue, b.reserved, amount));
        }
        b.reserved -= amount;
        b.in_flight += amount;
        tracing::debug!(venue = %venue, amount, reserved = b.reserved, in_flight = b.in_flight, "Treasury: commit");
        Ok(())
    }

    /// Fill confirmed: in_flight consumed
    pub fn fill(&self, venue: &str, amount: f64) -> Result<(), String> {
        let mut balances = self.balances.lock().unwrap();
        let b = balances.get_mut(venue).ok_or_else(|| format!("Unknown venue: {}", venue))?;
        b.in_flight -= amount;
        tracing::debug!(venue = %venue, amount, in_flight = b.in_flight, "Treasury: fill");
        Ok(())
    }

    /// Release reserved capital back: reserved → free
    pub fn release(&self, venue: &str, amount: f64) -> Result<(), String> {
        let mut balances = self.balances.lock().unwrap();
        let b = balances.get_mut(venue).ok_or_else(|| format!("Unknown venue: {}", venue))?;
        b.reserved -= amount;
        b.free += amount;
        tracing::debug!(venue = %venue, amount, free = b.free, reserved = b.reserved, "Treasury: release");
        Ok(())
    }

    /// Credit winnings: add to free
    pub fn credit(&self, venue: &str, amount: f64) -> Result<(), String> {
        let mut balances = self.balances.lock().unwrap();
        let b = balances.get_mut(venue).ok_or_else(|| format!("Unknown venue: {}", venue))?;
        b.free += amount;
        tracing::debug!(venue = %venue, amount, free = b.free, "Treasury: credit");
        Ok(())
    }

    pub fn get_free_balance(&self, venue: &str) -> f64 {
        let balances = self.balances.lock().unwrap();
        balances.get(venue).map(|b| b.free).unwrap_or(0.0)
    }

    pub fn get_all_free_balances(&self) -> HashMap<String, f64> {
        let balances = self.balances.lock().unwrap();
        balances.iter().map(|(k, v)| (k.clone(), v.free)).collect()
    }

    pub fn get_all_balances(&self) -> Vec<VenueBalance> {
        let balances = self.balances.lock().unwrap();
        balances.values().cloned().collect()
    }
}
