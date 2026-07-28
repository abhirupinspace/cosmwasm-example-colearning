//! # Errors — every way a call can be rejected
//!
//! In Solidity you write `require(cond, "message")`. In CosmWasm you return
//! `Err(ContractError::Something {})` and the whole transaction is rolled back,
//! exactly like a revert. Defining errors as an enum (instead of raw strings)
//! means tests can assert on the *exact* failure, not on message text.

use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    /// Wraps errors coming from CosmWasm itself — e.g. a malformed address
    /// passed to `addr_validate`, or a failed storage read.
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Only the chairperson can give the right to vote")]
    NotChairperson {},

    #[error("The ballot needs at least one proposal")]
    NoProposals {},

    #[error("Too many proposals: {count} (max {max})")]
    TooManyProposals { count: usize, max: usize },

    #[error("Proposal names must be unique; '{name}' appears more than once")]
    DuplicateProposal { name: String },

    #[error("Address {address} has no right to vote")]
    NoRightToVote { address: String },

    #[error("Address {address} already voted or delegated")]
    AlreadyVoted { address: String },

    #[error("Address {address} already has the right to vote")]
    AlreadyHasRightToVote { address: String },

    #[error("You cannot delegate to yourself")]
    SelfDelegation {},

    #[error("Found a loop in the delegation chain")]
    DelegationLoop {},

    #[error("Delegation chain is longer than {max} hops")]
    DelegationChainTooLong { max: u32 },

    #[error("The address you delegated to has no right to vote")]
    DelegateHasNoRightToVote {},

    #[error("No proposal at index {index}; there are {count}")]
    ProposalNotFound { index: u32, count: usize },

    #[error("Vote count overflowed")]
    Overflow {},
}
