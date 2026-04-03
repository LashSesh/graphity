// Copyright (c) 2026 Sebastian Klemm
// SPDX-License-Identifier: MIT
//
//! MERKABA Orbitkern — Backend-agnostic Ophanim Swarm.
//!
//! M1 replaces the single oracle call per LLM-node with a swarm of n calls,
//! each with a different context projection (Chameleon), aggregated through
//! structural resonance consensus (Ophanim + Konus), and collapsed into a
//! single emission via the Monolith Gate.
//!
//! # Architecture
//!
//! ```text
//! Prompt ──► Chameleon (4 lenses) ──► n Oracle calls ──► Ophanim (Dk)
//!                                                            │
//!                                                       Konus (Dtotal)
//!                                                            │
//!                                                   Monolith Gate ──► Emit
//! ```
//!
//! # Key Property
//!
//! `SwarmOracle` wraps any `Box<dyn Oracle>`. It does not know or care what
//! oracle it wraps. The consensus math is identical whether the inner oracle
//! is GPT-4o or a local 7B model. The intelligence is in the exoskeleton.

pub mod chameleon;
pub mod konus;
pub mod monolith;
pub mod ophanim;
pub mod swarm_oracle;

pub use swarm_oracle::SwarmOracle;
