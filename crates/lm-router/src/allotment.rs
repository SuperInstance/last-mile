//! Allotments and the exchange table: what a pool may spend, and what that
//! spend is worth.
//!
//! No provider will tell us what is left in the plan, so the router meters
//! **by observation**: it counts the tokens and the request the answer
//! reported, and it stops asking a pool when the pool's allotment for the
//! window is spent (fleet/01).
//!
//! The exchange table turns a pool's tokens into credits, so that pools
//! with different prices can be compared in one currency. Credits are kept
//! as integers: one credit is a thousandth of a credit in the meter, so
//! nothing here needs a float.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::SkipReason;
use crate::pool::Pool;

/// What a pool may spend in one window.
///
/// A zero means unlimited: the stub and the test pools are never limited.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Allotment {
    /// Tokens per window.
    #[serde(default)]
    pub tokens: u64,
    /// Requests per window.
    #[serde(default)]
    pub requests: u64,
}

impl Allotment {
    /// An allotment with no limit.
    pub fn unlimited() -> Allotment {
        Allotment::default()
    }

    /// Whether the allotment limits nothing.
    pub fn is_unlimited(&self) -> bool {
        self.tokens == 0 && self.requests == 0
    }
}

impl fmt::Display for Allotment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_unlimited() {
            return f.write_str("unlimited");
        }
        write!(f, "{} tokens, {} requests", self.tokens, self.requests)
    }
}

/// The exchange table: what a pool's tokens are worth in credits.
///
/// The rate is **credits per 1000 tokens**. The meter keeps credits in
/// thousandths, so a rate of `2` means 2000 tokens cost 4000 thousandths,
/// which is 4 credits: `tokens * rate`.
///
/// In `router.toml` it is a plain table of pool names:
///
/// ```toml
/// [exchange]
/// claude = 10
/// minimax = 1
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ExchangeTable {
    /// The rate of each pool, by pool name.
    rates: BTreeMap<String, u32>,
}

impl<'de> Deserialize<'de> for ExchangeTable {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<ExchangeTable, D::Error> {
        let rates = BTreeMap::<String, u32>::deserialize(d)?;
        Ok(ExchangeTable { rates })
    }
}

impl ExchangeTable {
    /// An empty table: every pool is free.
    pub fn new() -> ExchangeTable {
        ExchangeTable::default()
    }

    /// The rate of a pool, in credits per 1000 tokens. 0 when the pool is
    /// not in the table.
    pub fn rate(&self, pool: Pool) -> u32 {
        self.rates.get(pool.as_str()).copied().unwrap_or(0)
    }

    /// Set the rate of a pool.
    pub fn set(&mut self, pool: Pool, credits_per_1k_tokens: u32) {
        self.rates
            .insert(pool.as_str().to_string(), credits_per_1k_tokens);
    }

    /// What `tokens` cost at a pool's rate, in thousandths of a credit.
    pub fn milli_credits(&self, pool: Pool, tokens: u64) -> u64 {
        tokens.saturating_mul(self.rate(pool) as u64)
    }

    /// Every rate, by pool name.
    pub fn rates(&self) -> impl Iterator<Item = (&str, u32)> + '_ {
        self.rates.iter().map(|(k, v)| (k.as_str(), *v))
    }

    /// Whether the table holds no rates at all.
    pub fn is_empty(&self) -> bool {
        self.rates.is_empty()
    }
}

impl fmt::Display for ExchangeTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rates = self
            .rates()
            .map(|(name, rate)| format!("{name}={rate}"))
            .collect::<Vec<_>>()
            .join(", ");
        write!(f, "ExchangeTable({rates})")
    }
}

/// What a pool has spent in the current window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolUsage {
    /// Tokens spent.
    pub tokens: u64,
    /// Requests made.
    pub requests: u64,
    /// Credits spent, in thousandths of a credit.
    pub milli_credits: u64,
}

impl fmt::Display for PoolUsage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} tokens, {} requests, {} milli-credits",
            self.tokens, self.requests, self.milli_credits
        )
    }
}

/// The meter: what every pool has spent, and which pools are cold.
///
/// The window is a fixed window that starts when the meter is made. When it
/// ends, the counts go back to zero and a new window starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Meter {
    window_ms: u64,
    window_start_ms: u64,
    usage: BTreeMap<Pool, PoolUsage>,
    cold_until_ms: BTreeMap<Pool, u64>,
}

impl Meter {
    /// A meter whose window lasts `window_ms` and started at `now_ms`.
    pub fn new(window_ms: u64, now_ms: u64) -> Meter {
        Meter {
            window_ms,
            window_start_ms: now_ms,
            usage: BTreeMap::new(),
            cold_until_ms: BTreeMap::new(),
        }
    }

    /// How long the window lasts, in milliseconds.
    pub fn window_ms(&self) -> u64 {
        self.window_ms
    }

    /// When the current window started, in milliseconds.
    pub fn window_start_ms(&self) -> u64 {
        self.window_start_ms
    }

    /// Start a new window if the last one has ended.
    pub fn roll(&mut self, now_ms: u64) {
        if now_ms >= self.window_start_ms.saturating_add(self.window_ms) {
            self.window_start_ms = now_ms;
            self.usage.clear();
        }
    }

    /// What a pool has spent in the current window.
    pub fn usage(&self, pool: Pool) -> PoolUsage {
        self.usage.get(&pool).copied().unwrap_or_default()
    }

    /// Count one call against a pool: the tokens the answer reported and the
    /// credits those tokens are worth.
    pub fn observe(&mut self, pool: Pool, tokens: u64, milli_credits: u64, now_ms: u64) {
        self.roll(now_ms);
        let usage = self.usage.entry(pool).or_default();
        usage.tokens = usage.tokens.saturating_add(tokens);
        usage.requests = usage.requests.saturating_add(1);
        usage.milli_credits = usage.milli_credits.saturating_add(milli_credits);
    }

    /// Whether a pool's allotment for the window is spent, and why.
    pub fn exhausted(
        &mut self,
        pool: Pool,
        allotment: &Allotment,
        now_ms: u64,
    ) -> Option<SkipReason> {
        self.roll(now_ms);
        let usage = self.usage(pool);
        if allotment.requests > 0 && usage.requests >= allotment.requests {
            return Some(SkipReason::AllotmentRequests);
        }
        if allotment.tokens > 0 && usage.tokens >= allotment.tokens {
            return Some(SkipReason::AllotmentTokens);
        }
        None
    }

    /// Whether a pool is cold: it was rate limited and its cooldown has not
    /// ended.
    pub fn is_cold(&self, pool: Pool, now_ms: u64) -> bool {
        self.cold_until_ms
            .get(&pool)
            .is_some_and(|until| *until > now_ms)
    }

    /// Mark a pool cold until `now_ms + cooldown_ms`.
    pub fn mark_cold(&mut self, pool: Pool, now_ms: u64, cooldown_ms: u64) {
        let until = now_ms.saturating_add(cooldown_ms);
        let slot = self.cold_until_ms.entry(pool).or_insert(0);
        if until > *slot {
            *slot = until;
        }
    }

    /// When a pool's cooldown ends, if it is cold or has been cold.
    pub fn cold_until_ms(&self, pool: Pool) -> Option<u64> {
        self.cold_until_ms.get(&pool).copied()
    }

    /// How long until a pool is warm again, in milliseconds.
    pub fn cooldown_remaining_ms(&self, pool: Pool, now_ms: u64) -> u64 {
        self.cold_until_ms
            .get(&pool)
            .map(|until| until.saturating_sub(now_ms))
            .unwrap_or(0)
    }
}

impl fmt::Display for Meter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Meter(window {} ms, {} pools metered)",
            self.window_ms,
            self.usage.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_exchange_table_prices_tokens() {
        let mut table = ExchangeTable::new();
        table.set(Pool::Claude, 10);
        table.set(Pool::MiniMax, 1);
        // 2000 tokens at 10 credits per 1000 tokens is 20 credits.
        assert_eq!(table.milli_credits(Pool::Claude, 2000), 20_000);
        assert_eq!(table.milli_credits(Pool::MiniMax, 2000), 2_000);
        assert_eq!(table.rate(Pool::Zai), 0);
    }

    #[test]
    fn the_allotment_is_enforced_by_observation() {
        let mut meter = Meter::new(1000, 0);
        let allot = Allotment {
            tokens: 100,
            requests: 2,
        };
        assert_eq!(meter.exhausted(Pool::Zai, &allot, 0), None);
        meter.observe(Pool::Zai, 50, 50, 0);
        assert_eq!(meter.exhausted(Pool::Zai, &allot, 0), None);
        meter.observe(Pool::Zai, 50, 50, 0);
        assert_eq!(
            meter.exhausted(Pool::Zai, &allot, 0),
            Some(SkipReason::AllotmentRequests)
        );
    }

    #[test]
    fn the_token_allotment_stops_a_pool_too() {
        let mut meter = Meter::new(1000, 0);
        let allot = Allotment {
            tokens: 100,
            requests: 0,
        };
        meter.observe(Pool::Kimi, 100, 0, 0);
        assert_eq!(
            meter.exhausted(Pool::Kimi, &allot, 0),
            Some(SkipReason::AllotmentTokens)
        );
    }

    #[test]
    fn a_zero_allotment_is_unlimited() {
        let mut meter = Meter::new(1000, 0);
        let allot = Allotment::unlimited();
        meter.observe(Pool::Stub, 1_000_000, 0, 0);
        assert_eq!(meter.exhausted(Pool::Stub, &allot, 0), None);
        assert!(allot.is_unlimited());
    }

    #[test]
    fn the_window_ends_and_the_counts_go_back_to_zero() {
        let mut meter = Meter::new(1000, 0);
        meter.observe(Pool::Zai, 100, 100, 500);
        assert_eq!(meter.usage(Pool::Zai).tokens, 100);
        meter.roll(1500);
        assert_eq!(meter.usage(Pool::Zai).tokens, 0);
        assert_eq!(meter.window_start_ms(), 1500);
    }

    #[test]
    fn a_cold_pool_warms_up_after_its_cooldown() {
        let mut meter = Meter::new(1000, 0);
        meter.mark_cold(Pool::Zai, 100, 500);
        assert!(meter.is_cold(Pool::Zai, 100));
        assert!(meter.is_cold(Pool::Zai, 599));
        assert!(!meter.is_cold(Pool::Zai, 600));
        assert_eq!(meter.cooldown_remaining_ms(Pool::Zai, 200), 400);
    }
}
