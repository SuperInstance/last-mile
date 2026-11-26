//! The router: try the pools in order, meter what was spent, and never print
//! a key.
//!
//! The rules, in the order they are applied to each pool in the caller's
//! preference list:
//!
//! 1. **Not in the config** — skipped.
//! 2. **Cold** — it was rate limited and its cooldown has not ended. Skipped.
//! 3. **No key** — the key is not in the secrets file, so the pool is
//!    unavailable. Skipped. (A missing key is never an error at start-up.)
//! 4. **Allotment spent** — the pool has used its window's tokens or
//!    requests. Skipped.
//! 5. **Called.** A 429 or a "limit" error makes the pool cold and the
//!    walk continues with the next pool.
//!
//! When the walk ends without an answer: every pool cold means
//! [`RouterError::AllCold`], a failure means that failure, and everything
//! else means [`RouterError::NoUsablePool`].

use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::adapters::{build_call, mentions_limit, parse_response};
use crate::allotment::{Meter, PoolUsage};
use crate::chat::{ChatReq, ChatResp, Usage};
use crate::config::{PoolConfig, ProviderKind, RouterConfig};
use crate::error::{RouterError, SkipReason};
use crate::pool::Pool;
use crate::secrets::Secrets;

/// What the stub answers when the config does not say.
pub const STUB_TEXT: &str = "stub: no provider answered";

/// How much of a provider's error body is kept.
const MAX_BODY: usize = 400;

/// The router: the only way money leaves the house.
pub struct Router {
    config: RouterConfig,
    secrets: Secrets,
    meter: Meter,
    agent: ureq::Agent,
}

/// How one pool ended the walk.
enum Step {
    /// The pool is cold: it was already cold, or it was rate limited.
    Cold,
    /// The pool was not used, for a reason of its own.
    Skipped,
    /// The pool was called and failed.
    Failed,
}

impl Router {
    /// A router with a config and its keys.
    pub fn new(config: RouterConfig, secrets: Secrets) -> Router {
        let meter = Meter::new(config.router.window_ms(), now_ms());
        // The status is read here, not turned into an error by ureq: a 429
        // is a cold pool, not a crash.
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        Router {
            config,
            secrets,
            meter,
            agent,
        }
    }

    /// A router that reads its keys from the config's secrets path.
    pub fn load(config: RouterConfig) -> Result<Router, RouterError> {
        let secrets = Secrets::from_file(&config.router.secrets_path())?;
        Ok(Router::new(config, secrets))
    }

    /// Ask a pool, trying the pools in the order given.
    ///
    /// The first pool that answers wins. A pool that is cold, has no key or
    /// has spent its allotment is skipped; a pool that fails is remembered
    /// and the walk continues.
    pub fn ask(&mut self, pool_pref: &[Pool], req: ChatReq) -> Result<ChatResp, RouterError> {
        if pool_pref.is_empty() {
            return Err(RouterError::NoPools);
        }
        let now = now_ms();
        let mut steps: Vec<(Pool, Step)> = Vec::new();
        let mut reasons: Vec<(Pool, SkipReason)> = Vec::new();
        let mut failures: Vec<RouterError> = Vec::new();

        for &pool in pool_pref {
            let Some(cfg) = self.config.pool(pool).cloned() else {
                reasons.push((pool, SkipReason::NotConfigured));
                steps.push((pool, Step::Skipped));
                continue;
            };
            if self.meter.is_cold(pool, now) {
                reasons.push((pool, SkipReason::Cold));
                steps.push((pool, Step::Cold));
                continue;
            }
            if let Some(name) = &cfg.key_name {
                if self.secrets.get(name).is_none() {
                    reasons.push((pool, SkipReason::NoKey));
                    steps.push((pool, Step::Skipped));
                    continue;
                }
            }
            if let Some(reason) = self.meter.exhausted(pool, &cfg.allotment, now) {
                reasons.push((pool, reason));
                steps.push((pool, Step::Skipped));
                continue;
            }

            let key = cfg
                .key_name
                .as_ref()
                .and_then(|name| self.secrets.get(name))
                .map(|secret| secret.expose().to_string());
            match self.call(pool, &cfg, key.as_deref(), &req, now) {
                Ok(resp) => return Ok(resp),
                Err(err) => {
                    if let RouterError::RateLimited { .. } = &err {
                        self.meter.mark_cold(pool, now, cfg.cooldown_ms);
                        steps.push((pool, Step::Cold));
                    } else {
                        steps.push((pool, Step::Failed));
                    }
                    failures.push(err);
                }
            }
        }

        // Nothing answered: say the most useful thing.
        if !steps.is_empty() && steps.iter().all(|(_, step)| matches!(step, Step::Cold)) {
            let pools = steps.iter().map(|(pool, _)| *pool).collect();
            let causes = failures
                .iter()
                .filter_map(|err| match err {
                    RouterError::RateLimited { pool, status, body } => {
                        Some(format!("{pool}: status {status}: {body}"))
                    }
                    _ => None,
                })
                .collect();
            return Err(RouterError::AllCold { pools, causes });
        }
        if let Some(err) = failures.into_iter().next() {
            return Err(err);
        }
        Err(RouterError::NoUsablePool { reasons })
    }

    /// Call one pool.
    fn call(
        &mut self,
        pool: Pool,
        cfg: &PoolConfig,
        key: Option<&str>,
        req: &ChatReq,
        now: u64,
    ) -> Result<ChatResp, RouterError> {
        if cfg.kind == ProviderKind::Stub {
            let text = cfg
                .stub_text
                .clone()
                .unwrap_or_else(|| STUB_TEXT.to_string());
            // The stub costs a request and no tokens.
            self.meter.observe(pool, 0, 0, now);
            return Ok(ChatResp {
                pool,
                model: cfg.model.clone(),
                text,
                usage: Usage::default(),
                finish_reason: None,
                confidence: None,
            });
        }

        let model = req.model.clone().unwrap_or_else(|| cfg.model.clone());
        let call = build_call(pool, cfg, req, key);
        let timeout = Duration::from_millis(cfg.timeout_ms);
        let mut builder = self.agent.post(call.url.as_str());
        for (name, value) in &call.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        let sent = builder
            .config()
            .timeout_global(Some(timeout))
            .build()
            .send_json(&call.body);

        let response = match sent {
            Ok(response) => response,
            Err(ureq::Error::Timeout(_)) => {
                return Err(RouterError::Timeout {
                    pool,
                    timeout_ms: cfg.timeout_ms,
                })
            }
            Err(err) => {
                return Err(RouterError::Transport {
                    pool,
                    why: self.scrub(&err.to_string()),
                })
            }
        };

        let status = response.status().as_u16();
        let body = response.into_body().read_to_string().unwrap_or_default();
        if !(200..300).contains(&status) {
            let scrubbed = self.scrub(&truncate(&body, MAX_BODY));
            if status == 429 || mentions_limit(&body) {
                return Err(RouterError::RateLimited {
                    pool,
                    status,
                    body: scrubbed,
                });
            }
            return Err(RouterError::Http {
                pool,
                status,
                body: scrubbed,
            });
        }

        let value: serde_json::Value =
            serde_json::from_str(&body).map_err(|e| RouterError::Malformed {
                pool,
                why: self.scrub(&e.to_string()),
            })?;
        let resp = parse_response(pool, cfg.kind, &model, &value)?;
        let tokens = resp.usage.total();
        let milli_credits = self.config.exchange.milli_credits(pool, tokens);
        self.meter.observe(pool, tokens, milli_credits, now);
        Ok(resp)
    }

    /// The router's config.
    pub fn config(&self) -> &RouterConfig {
        &self.config
    }

    /// The keys the router holds. Their values are not printable.
    pub fn secrets(&self) -> &Secrets {
        &self.secrets
    }

    /// The meter: what each pool has spent.
    pub fn meter(&self) -> &Meter {
        &self.meter
    }

    /// What a pool has spent in the current window.
    pub fn usage(&self, pool: Pool) -> PoolUsage {
        self.meter.usage(pool)
    }

    /// Whether a pool is cold right now.
    pub fn is_cold(&self, pool: Pool) -> bool {
        self.meter.is_cold(pool, now_ms())
    }

    /// Remove every key from a piece of text.
    fn scrub(&self, text: &str) -> String {
        self.secrets.scrub(text)
    }
}

impl Default for Router {
    fn default() -> Router {
        Router::new(RouterConfig::default_config(), Secrets::default())
    }
}

impl fmt::Debug for Router {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Router")
            .field("pools", &self.config.pool_names().collect::<Vec<_>>())
            .field("keys", &self.secrets.names().collect::<Vec<_>>())
            .field("meter", &self.meter)
            .finish()
    }
}

impl fmt::Display for Router {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Router({})",
            self.config.pool_names().collect::<Vec<_>>().join(", ")
        )
    }
}

/// The clock, in milliseconds since the epoch.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Shorten a piece of text without cutting a character in half.
fn truncate(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\u{2026}", &text[..end])
}
