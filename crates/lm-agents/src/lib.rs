//! `lm-agents`: how last-mile agents think (M1 phase 3).
//!
//! The pipeline, per agent:
//!
//! ```text
//! ScoreView --facts--> plain-language facts (code computes every number, D49)
//!           --gates--> typed System One questions over the sim's menu (D37)
//!           --sysone--> a decision model (d1-2B on the Thor by default) -> probabilities
//!           --queue--> least-use-first scheduling under budgets (D16)
//!           --mind--> scheduled Lane/Intent decisions, or a logged refusal (D26)
//! ```
//!
//! Nothing here is needed to replay a round: model answers reach the sim
//! only as logged decisions. See docs/superpowers/plans/2026-10-28-m1-phase3-agents.md.

pub mod agents;
pub use agents::{AgentStats, Agents, AskTiming, WordCounts};
pub mod config;
pub use config::AgentsConfig;
pub mod facts;
pub mod gates;
pub use gates::Pack;
pub mod harness;
pub use harness::Harness;
pub mod mind;
pub use mind::{
    Ask, AskOutcome, Mind, Mode, Policy, RttEstimator, Thinker, Thought, Tier, TierTable,
};
pub mod phrasebook;
pub use phrasebook::{Phrasebook, PhrasebookError};
pub mod queue;
pub use queue::{Budgets, LeastUseQueue, Popped, ThinkReq};
pub mod shared;
pub use shared::{GhostConfig, GhostPhase, SharedConfig, SharedRule, SharedTarget};
pub mod sink;
pub use sink::{ChannelSink, DecisionInbox, DecisionSink};
pub mod surprise;
pub use surprise::{RoundSurprise, SituationCounts, SituationKey};
pub mod sysone;
pub use sysone::{
    Answer, Answers, EscalatingBackend, Escalation, EscalationOutcome, Refusal, SysOne,
};
pub mod utter;
pub use utter::{
    pick_realizer, pick_realizer_with, AutoRealizer, GateCtx, Llm, LlmRealizer, Realizer, StubLlm,
    TemplateRealizer, UtterPlan, WordSource, Worded,
};
