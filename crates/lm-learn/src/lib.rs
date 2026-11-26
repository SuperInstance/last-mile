//! `lm-learn`: how squads get better between rounds, honestly (M1 phase 5).
//!
//! ```text
//! RoundLog --metrics--> what happened (incl. the model-vs-code dial, D40)
//!          --review--> a System-2 proposal: a gate, block, reflex, lexicon or policy edit (D47)
//!          --ledger--> a SEALED prediction of its effect, before any test (D32)
//!          --eval--> held-out seeds: baseline vs proposal
//!          --ledger--> the verdict, appended to the same chain
//!          --> adopted changes are committed to the agent's harness and pinned (D27)
//! ```
//!
//! The Vibe pipeline (`vibe`) is the same discipline on the other side of
//! the game: a kid's words become one typed change out of a closed
//! dictionary, its prediction is sealed before the test, and the verdict
//! adopts it only if every measure held (the sandbox, M2b). The vibe gate
//! (`hear`) is the first arrow of that: the words become the entry and
//! the value through two closed questions the decision server answers.
//!
//! Plan: docs/superpowers/plans/2026-10-28-m1-phase5-learning.md.
//! Modules are declared up front so parallel tasks never collide here.

pub mod deposits;
pub mod eval;
pub mod flow;
pub mod hear;
pub mod ledger;
pub mod metrics;
pub mod review;
pub mod round;
pub mod sign;
pub mod vibe;
