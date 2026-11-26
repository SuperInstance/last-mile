//! `lm-router`: the cloud purse (M1 phase 5, Task 2).
//!
//! ```text
//! secrets.env --keys--> Router --config(router.toml)--> pool: Z.AI | MiniMax | Kimi | Claude | Jev | Stub
//!                                  |
//!                                  +-- ask(pool_pref, req): try each pool in order
//!                                        |- missing key   -> that pool is unavailable, try the next
//!                                        |- allotment out -> that pool is unavailable, try the next
//!                                        |- 429 / "limit" -> the pool goes COLD until its cooldown
//!                                        `- all cold      -> RouterError::AllCold
//! ```
//!
//! Everything that spends money goes through here (D: "cloud spending goes
//! through `lm-router` only"). The real provider limits live in the config,
//! never in the agents' view. Every call is metered **by observation**: the
//! tokens and the request are counted from the answer, because no provider
//! will tell us what is left (fleet/01).
//!
//! Rules this crate keeps:
//!
//! - **Keys are read at runtime** from `~/.config/last-mile/secrets.env`
//!   (`NAME=value` lines). A missing key makes that pool *unavailable*; it
//!   is not an error at startup.
//! - **Keys are never printed.** Every type that can hold a key has a
//!   redacting `Debug`, and the error types never carry key material.
//! - **Base URLs, model names, allotments and the exchange table live in
//!   `router.toml`**, not in Rust source and never in a secret file.
//!
//! Plan: docs/superpowers/plans/2026-10-28-m1-phase5-learning.md.

pub mod adapters;
pub mod allotment;
pub mod chat;
pub mod config;
pub mod error;
pub mod pool;
pub mod router;
pub mod secrets;

pub use adapters::{build_call, parse_response, HttpCall};
pub use allotment::{Allotment, ExchangeTable, Meter, PoolUsage};
pub use chat::{ChatMessage, ChatReq, ChatResp, Role, Usage};
pub use config::{PoolConfig, ProviderKind, RouterConfig, RouterSettings};
pub use error::{RouterError, SkipReason};
pub use pool::Pool;
pub use router::Router;
pub use secrets::{Secret, Secrets};
