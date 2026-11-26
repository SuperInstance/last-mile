//! The pools: the places a cloud call can go.
//!
//! A pool is a provider *and* a purse. The provider decides the dialect the
//! pool speaks (see [`crate::adapters`]); the purse decides how much the pool
//! may spend (see [`crate::allotment`]). Which pool answers is never decided
//! here: the caller passes a preference list to
//! [`Router::ask`](crate::router::Router::ask) and the router walks it.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::RouterError;

/// A cloud provider pool, or the offline stub.
///
/// The order of the variants is the order the pools are listed in
/// `router.toml`. It is never used to pick a provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pool {
    /// Z.AI (GLM): OpenAI-compatible chat completions.
    Zai,
    /// MiniMax: OpenAI-compatible chat completions.
    MiniMax,
    /// Kimi (Moonshot): OpenAI-compatible chat completions.
    Kimi,
    /// Anthropic Claude: the Messages API.
    Claude,
    /// Jev: System One.
    Jev,
    /// The stub: no network, no key, no cost.
    Stub,
}

impl Pool {
    /// Every pool, in config order.
    pub const ALL: [Pool; 6] = [
        Pool::Zai,
        Pool::MiniMax,
        Pool::Kimi,
        Pool::Claude,
        Pool::Jev,
        Pool::Stub,
    ];

    /// The pool's name, as it appears in `router.toml`.
    pub fn as_str(self) -> &'static str {
        match self {
            Pool::Zai => "zai",
            Pool::MiniMax => "minimax",
            Pool::Kimi => "kimi",
            Pool::Claude => "claude",
            Pool::Jev => "jev",
            Pool::Stub => "stub",
        }
    }

    /// The name of the key this pool needs in the secrets file.
    ///
    /// The stub needs none: it never leaves the machine.
    pub fn key_name(self) -> &'static str {
        match self {
            Pool::Zai => "ZAI_API_KEY",
            Pool::MiniMax => "MINIMAX_API_KEY",
            Pool::Kimi => "KIMI_API_KEY",
            Pool::Claude => "ANTHROPIC_API_KEY",
            Pool::Jev => "TYPESAFE_API_KEY",
            Pool::Stub => "",
        }
    }

    /// Parse a pool name. The common aliases of a provider are accepted, so
    /// a config or a command line can say `anthropic` or `z.ai`.
    pub fn parse(name: &str) -> Result<Pool, RouterError> {
        match name.trim().to_ascii_lowercase().as_str() {
            "zai" | "z.ai" | "z-ai" | "glm" => Ok(Pool::Zai),
            "minimax" | "mini-max" => Ok(Pool::MiniMax),
            "kimi" | "moonshot" => Ok(Pool::Kimi),
            "claude" | "anthropic" => Ok(Pool::Claude),
            "jev" | "typesafe" | "systemone" | "system-one" => Ok(Pool::Jev),
            "stub" | "offline" => Ok(Pool::Stub),
            other => Err(RouterError::Config {
                why: format!("unknown pool name: {other}"),
            }),
        }
    }
}

impl fmt::Display for Pool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Pool {
    type Err = RouterError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Pool::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for pool in Pool::ALL {
            assert_eq!(Pool::parse(pool.as_str()).unwrap(), pool);
        }
    }

    #[test]
    fn aliases_are_accepted() {
        assert_eq!(Pool::parse("Z.AI").unwrap(), Pool::Zai);
        assert_eq!(Pool::parse("Anthropic").unwrap(), Pool::Claude);
        assert_eq!(Pool::parse("moonshot").unwrap(), Pool::Kimi);
        assert_eq!(Pool::parse("MiniMax").unwrap(), Pool::MiniMax);
    }

    #[test]
    fn unknown_pool_is_a_config_error() {
        let err = Pool::parse("llama").unwrap_err();
        assert!(matches!(err, RouterError::Config { .. }));
        assert!(err.to_string().contains("llama"));
    }

    #[test]
    fn every_real_pool_names_a_key() {
        for pool in Pool::ALL {
            let name = pool.key_name();
            if pool == Pool::Stub {
                assert!(name.is_empty());
            } else {
                assert!(name.ends_with("_API_KEY"));
            }
        }
    }
}
