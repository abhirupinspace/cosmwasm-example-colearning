//! # A ballot contract for Injective
//!
//! This is the classic Solidity `Ballot.sol` teaching contract, rewritten in
//! Rust for CosmWasm. It runs on Injective, which uses stock CosmWasm — nothing
//! Injective-specific is needed.
//!
//! ## How it works, in four sentences
//!
//! Whoever deploys the contract becomes the **chairperson** and gets one vote.
//! The chairperson hands out the right to vote, one address at a time; each
//! grant is worth one vote. A voter either **votes** for a proposal or
//! **delegates** their vote to someone else — one or the other, once, forever.
//! Anyone can read the running totals for free.
//!
//! ## Where to start reading
//!
//! 1. [`msg`] — the public API. What you can send, and what comes back.
//! 2. [`state`] — what the contract stores on-chain.
//! 3. [`contract`] — the logic. Start at `instantiate` and read downwards.
//! 4. [`error`] — every way a call can be rejected.
//!
//! `tests/integration.rs` runs all of it against a simulated blockchain, and
//! doubles as a plain-English spec of the rules.

pub mod contract;
mod error;
pub mod msg;
pub mod state;

pub use crate::error::ContractError;
