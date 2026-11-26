//! What can go wrong, and why nothing here ever holds a key.
//!
//! Every error is built from things the router already knows: a pool, a
//! status code, a (scrubbed) body. The key itself never enters an error,
//! and no error's `Debug` or `Display` can print one.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::pool::Pool;

/// Why a pool was not used for a call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// The pool is cold: it was rate limited and its cooldown has not ended.
    Cold,
    /// The pool has no key in the secrets file, so it is unavailable.
    NoKey,
    /// The pool's token allotment for the window is spent.
    AllotmentTokens,
    /// The pool's request allotment for the window is spent.
    AllotmentRequests,
    /// The pool is not in the config.
    NotConfigured,
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            SkipReason::Cold => "cold",
            SkipReason::NoKey => "no key",
            SkipReason::AllotmentTokens => "token allotment spent",
            SkipReason::AllotmentRequests => "request allotment spent",
            SkipReason::NotConfigured => "not configured",
        };
        f.write_str(s)
    }
}

/// Everything the router can fail with.
#[derive(Debug, thiserror::Error)]
pub enum RouterError {
    /// The preference list was empty: there was nowhere to send the call.
    #[error("no pools in the preference list")]
    NoPools,

    /// Every pool in the preference list was cold (rate limited) when the
    /// call came. The review is skipped, and the round goes on.
    #[error("every pool is cold: {}{}", crate::error::join_pools(.pools), crate::error::join_causes(.causes))]
    AllCold {
        /// The pools that were cold.
        pools: Vec<Pool>,
        /// What the router saw that made them cold, when it saw it.
        causes: Vec<String>,
    },

    /// No pool could be used, and none of them failed: they were skipped
    /// for a reason of their own (no key, allotment spent, not configured).
    #[error("no usable pool: {}", crate::error::join_reasons(.reasons))]
    NoUsablePool {
        /// Why each pool was skipped.
        reasons: Vec<(Pool, SkipReason)>,
    },

    /// The provider refused the call for quota reasons: a 429, or an error
    /// body that says "limit". The pool goes cold until its cooldown ends.
    #[error("pool {pool} is rate limited (status {status}): {body}")]
    RateLimited {
        /// The pool that refused.
        pool: Pool,
        /// The status it answered with.
        status: u16,
        /// Its (scrubbed) answer.
        body: String,
    },

    /// The provider answered with a status that is not 2xx.
    #[error("pool {pool} answered {status}: {body}")]
    Http {
        /// The pool that answered.
        pool: Pool,
        /// The status it answered with.
        status: u16,
        /// Its (scrubbed) answer.
        body: String,
    },

    /// The provider did not answer in time.
    #[error("pool {pool} timed out after {timeout_ms} ms")]
    Timeout {
        /// The pool that went quiet.
        pool: Pool,
        /// How long we waited.
        timeout_ms: u64,
    },

    /// The call never reached the provider: the connection failed, the name
    /// did not resolve, TLS did.
    #[error("pool {pool} could not be reached: {why}")]
    Transport {
        /// The pool we tried.
        pool: Pool,
        /// What went wrong.
        why: String,
    },

    /// The provider answered with something that is not a chat answer.
    #[error("pool {pool} answered with something unreadable: {why}")]
    Malformed {
        /// The pool that answered.
        pool: Pool,
        /// What could not be read.
        why: String,
    },

    /// The router's own config is missing or broken.
    #[error("router config: {why}")]
    Config {
        /// What is wrong with the config.
        why: String,
    },

    /// The secrets file could not be read.
    #[error("secrets: {why}")]
    Secrets {
        /// What went wrong.
        why: String,
    },
}

impl RouterError {
    /// The pool this error is about, when it is about one.
    pub fn pool(&self) -> Option<Pool> {
        match self {
            RouterError::RateLimited { pool, .. }
            | RouterError::Http { pool, .. }
            | RouterError::Timeout { pool, .. }
            | RouterError::Transport { pool, .. }
            | RouterError::Malformed { pool, .. } => Some(*pool),
            _ => None,
        }
    }
}

/// `zai, minimax` — the pool list of an error, for its message.
pub(crate) fn join_pools(pools: &[Pool]) -> String {
    if pools.is_empty() {
        return "none".to_string();
    }
    pools
        .iter()
        .map(|p| p.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// `zai (cold), kimi (no key)` — why each pool was skipped, for an error.
pub(crate) fn join_reasons(reasons: &[(Pool, SkipReason)]) -> String {
    if reasons.is_empty() {
        return "none".to_string();
    }
    reasons
        .iter()
        .map(|(pool, reason)| format!("{pool} ({reason})"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `; zai: status 429: ...` — what made the pools cold, for an error.
pub(crate) fn join_causes(causes: &[String]) -> String {
    if causes.is_empty() {
        return String::new();
    }
    format!("; {}", causes.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_name_their_pool() {
        let err = RouterError::Timeout {
            pool: Pool::Kimi,
            timeout_ms: 250,
        };
        assert_eq!(err.pool(), Some(Pool::Kimi));
        assert!(err.to_string().contains("kimi"));
        assert!(err.to_string().contains("250"));
    }

    #[test]
    fn all_cold_lists_every_pool() {
        let err = RouterError::AllCold {
            pools: vec![Pool::Zai, Pool::MiniMax],
            causes: vec!["zai: status 429: rate limit".to_string()],
        };
        let text = err.to_string();
        assert!(text.contains("zai"));
        assert!(text.contains("minimax"));
        assert!(text.contains("429"));
        assert_eq!(err.pool(), None);
    }

    #[test]
    fn no_usable_pool_says_why() {
        let err = RouterError::NoUsablePool {
            reasons: vec![
                (Pool::Zai, SkipReason::NoKey),
                (Pool::Claude, SkipReason::AllotmentTokens),
            ],
        };
        let text = err.to_string();
        assert!(text.contains("zai (no key)"));
        assert!(text.contains("claude (token allotment spent)"));
    }
}
