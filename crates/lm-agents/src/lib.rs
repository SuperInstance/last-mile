//! `lm-agents`: how last-mile agents think (M1 phase 3).
//!
//! The pipeline, per agent:
//!
//! ```text
//! ScoreView --facts--> plain-language facts (code computes every number, D49)
//!           --gates--> typed System One questions over the sim's menu (D37)
//!           --sysone--> a decision model (d1-3B on the Thor by default) -> probabilities
//!           --queue--> least-use-first scheduling under budgets (D16)
//!           --mind--> scheduled Lane/Intent decisions, or a logged refusal (D26)
//! ```
//!
//! Nothing here is needed to replay a round: model answers reach the sim
//! only as logged decisions. See docs/superpowers/plans/2026-10-28-m1-phase3-agents.md.

pub mod config;
pub mod facts;
pub mod gates;
pub mod queue;
pub mod sysone;
