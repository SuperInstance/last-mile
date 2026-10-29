//! `router.toml`: the base URLs, the model names, the allotments and the
//! exchange table.
//!
//! Everything the router needs to know about a provider that is not a secret
//! lives in this file, so the limits of the cloud subscriptions are changed
//! without touching Rust source — and never reach an agent (D: "real limits
//! live only in `lm-router`").
//!
//! Keys never live here. They are read at runtime from the secrets file
//! named under `[router]`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::allotment::{Allotment, ExchangeTable};
use crate::error::RouterError;
use crate::pool::Pool;

/// The config that ships with the crate.
pub const DEFAULT_CONFIG: &str = include_str!("../assets/router.toml");

/// Where the secrets live, unless the config says otherwise.
pub const DEFAULT_SECRETS_PATH: &str = "~/.config/last-mile/secrets.env";

/// Where the config lives, unless something else is asked for.
pub const DEFAULT_CONFIG_PATH: &str = "~/.config/last-mile/router.toml";

/// The dialect a pool speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// The OpenAI chat-completions shape: Z.AI, MiniMax and Kimi.
    OpenAi,
    /// The Anthropic Messages shape: Claude.
    Anthropic,
    /// The System One shape: Jev.
    SystemOne,
    /// No network at all: the stub.
    Stub,
}

/// One pool's settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolConfig {
    /// Which dialect the pool speaks.
    pub kind: ProviderKind,
    /// The provider's base URL. The endpoint's path follows from the kind:
    /// `/chat/completions`, `/v1/messages` or `/v1/systemone`.
    #[serde(default)]
    pub base_url: String,
    /// The model to ask for.
    #[serde(default)]
    pub model: String,
    /// The name of the key in the secrets file. `None` means the pool needs
    /// no key (the stub).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_name: Option<String>,
    /// How long to wait for the pool, in milliseconds.
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    /// How long the pool stays cold after a rate-limit error, in
    /// milliseconds.
    #[serde(default = "default_cooldown_ms")]
    pub cooldown_ms: u64,
    /// What the pool may spend in one window. Zero means unlimited.
    #[serde(default)]
    pub allotment: Allotment,
    /// What the stub answers, when the pool is the stub.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stub_text: Option<String>,
}

fn default_timeout_ms() -> u64 {
    20_000
}

fn default_cooldown_ms() -> u64 {
    60_000
}

impl Default for PoolConfig {
    fn default() -> PoolConfig {
        PoolConfig {
            kind: ProviderKind::OpenAi,
            base_url: String::new(),
            model: String::new(),
            key_name: None,
            timeout_ms: default_timeout_ms(),
            cooldown_ms: default_cooldown_ms(),
            allotment: Allotment::unlimited(),
            stub_text: None,
        }
    }
}

/// The router's own settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouterSettings {
    /// Where the keys are read from. `~` is expanded.
    #[serde(default = "default_secrets_path")]
    pub secrets_path: String,
    /// How long an allotment window lasts, in seconds.
    #[serde(default = "default_window_seconds")]
    pub window_seconds: u64,
}

fn default_secrets_path() -> String {
    DEFAULT_SECRETS_PATH.to_string()
}

fn default_window_seconds() -> u64 {
    3600
}

impl Default for RouterSettings {
    fn default() -> RouterSettings {
        RouterSettings {
            secrets_path: default_secrets_path(),
            window_seconds: default_window_seconds(),
        }
    }
}

impl RouterSettings {
    /// Where the keys are, with `~` expanded.
    pub fn secrets_path(&self) -> PathBuf {
        expand_tilde(&self.secrets_path)
    }

    /// The window's length in milliseconds.
    pub fn window_ms(&self) -> u64 {
        self.window_seconds.saturating_mul(1000)
    }
}

/// The whole router config.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouterConfig {
    /// The router's own settings.
    #[serde(default)]
    pub router: RouterSettings,
    /// The pools, by name.
    #[serde(default)]
    pub pools: BTreeMap<String, PoolConfig>,
    /// The exchange table.
    #[serde(default)]
    pub exchange: ExchangeTable,
}

impl RouterConfig {
    /// The config that ships with the crate.
    pub fn default_config() -> RouterConfig {
        RouterConfig::from_toml_str(DEFAULT_CONFIG).expect("the shipped router.toml is valid")
    }

    /// Read a config from TOML.
    pub fn from_toml_str(text: &str) -> Result<RouterConfig, RouterError> {
        let config: RouterConfig =
            toml::from_str(text).map_err(|e| RouterError::Config { why: e.to_string() })?;
        config.check()?;
        Ok(config)
    }

    /// Read a config from a file.
    pub fn load(path: &Path) -> Result<RouterConfig, RouterError> {
        let text = fs::read_to_string(path).map_err(|e| RouterError::Config {
            why: format!("{}: {e}", path.display()),
        })?;
        RouterConfig::from_toml_str(&text)
    }

    /// The user's config if there is one, else the shipped config.
    ///
    /// A broken user config is an error: silently falling back to the
    /// shipped one would spend the wrong money.
    pub fn discover() -> Result<RouterConfig, RouterError> {
        let path = expand_tilde(DEFAULT_CONFIG_PATH);
        if path.exists() {
            RouterConfig::load(&path)
        } else {
            Ok(RouterConfig::default_config())
        }
    }

    /// The settings of one pool, if it is in the config.
    pub fn pool(&self, pool: Pool) -> Option<&PoolConfig> {
        self.pools.get(pool.as_str())
    }

    /// The names of the pools in the config, in order.
    pub fn pool_names(&self) -> impl Iterator<Item = &str> {
        self.pools.keys().map(String::as_str)
    }

    /// Complain about a config that cannot work.
    fn check(&self) -> Result<(), RouterError> {
        for (name, cfg) in &self.pools {
            Pool::parse(name).map_err(|_| RouterError::Config {
                why: format!("unknown pool in the config: {name}"),
            })?;
            if cfg.kind != ProviderKind::Stub && cfg.base_url.is_empty() {
                return Err(RouterError::Config {
                    why: format!("pool {name} has no base_url"),
                });
            }
            if cfg.kind != ProviderKind::Stub && cfg.model.is_empty() {
                return Err(RouterError::Config {
                    why: format!("pool {name} has no model"),
                });
            }
        }
        Ok(())
    }
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            ProviderKind::OpenAi => "openai",
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::SystemOne => "systemone",
            ProviderKind::Stub => "stub",
        };
        f.write_str(name)
    }
}

impl std::fmt::Display for PoolConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PoolConfig({}, {}, {} tokens, {} requests)",
            self.kind, self.model, self.allotment.tokens, self.allotment.requests
        )
    }
}

impl std::fmt::Display for RouterSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "RouterSettings({}, {} s windows)",
            self.secrets_path, self.window_seconds
        )
    }
}

impl std::fmt::Display for RouterConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "RouterConfig({})",
            self.pool_names().collect::<Vec<_>>().join(", ")
        )
    }
}

/// Expand a leading `~` to the home directory.
pub fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        return home_dir();
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return home_dir().join(rest);
    }
    PathBuf::from(path)
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_config_is_valid() {
        let config = RouterConfig::default_config();
        for pool in Pool::ALL {
            assert!(config.pool(pool).is_some(), "{pool} is missing");
        }
        assert!(config.exchange.rate(Pool::Claude) > 0);
        assert_eq!(config.router.window_seconds, 3600);
    }

    #[test]
    fn a_pool_is_read_from_its_table() {
        let config = RouterConfig::from_toml_str(
            r#"
[pools.zai]
kind = "openai"
base_url = "https://example.invalid/v1"
model = "glm-test"
key_name = "ZAI_API_KEY"

[pools.zai.allotment]
tokens = 1000
requests = 10
"#,
        )
        .unwrap();
        let zai = config.pool(Pool::Zai).unwrap();
        assert_eq!(zai.kind, ProviderKind::OpenAi);
        assert_eq!(zai.model, "glm-test");
        assert_eq!(zai.key_name.as_deref(), Some("ZAI_API_KEY"));
        assert_eq!(zai.allotment.tokens, 1000);
        assert_eq!(zai.allotment.requests, 10);
        assert!(config.pool(Pool::Claude).is_none());
    }

    #[test]
    fn a_pool_without_a_base_url_is_a_config_error() {
        let err = RouterConfig::from_toml_str(
            r#"
[pools.zai]
kind = "openai"
model = "glm-test"
"#,
        )
        .unwrap_err();
        assert!(matches!(err, RouterError::Config { .. }));
        assert!(err.to_string().contains("base_url"));
    }

    #[test]
    fn an_unknown_pool_name_is_a_config_error() {
        let err = RouterConfig::from_toml_str(
            r#"
[pools.llama]
kind = "openai"
base_url = "https://example.invalid"
model = "llama"
"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("llama"));
    }

    #[test]
    fn a_tilde_is_expanded() {
        let path = expand_tilde("~/.config/last-mile/secrets.env");
        assert!(!path.starts_with("~"));
        assert!(path.ends_with("last-mile/secrets.env"));
    }
}
