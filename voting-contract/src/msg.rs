//! # Messages — the contract's public API
//!
//! Solidity contracts expose *functions*. CosmWasm contracts expose exactly
//! three entry points — `instantiate`, `execute`, `query` — and you choose
//! which "function" to run by sending a JSON message that names it.
//!
//! So this file is the CosmWasm version of a Solidity ABI:
//!
//! | Solidity                      | CosmWasm                                  |
//! |-------------------------------|-------------------------------------------|
//! | `constructor(names)`          | `InstantiateMsg`                          |
//! | `giveRightToVote(voter)`      | `ExecuteMsg::GiveRightToVote { voter }`   |
//! | `vote(proposal)`              | `ExecuteMsg::Vote { proposal }`           |
//! | `delegate(to)`                | `ExecuteMsg::Delegate { to }`             |
//! | `winningProposal()` (view)    | `QueryMsg::WinningProposal {}`            |
//! | `winnerName()` (view)         | `QueryMsg::WinnerName {}`                 |
//!
//! `#[cw_serde]` turns each enum variant into snake_case JSON. For example
//! `ExecuteMsg::GiveRightToVote { voter }` is sent over the wire as:
//!
//! ```json
//! { "give_right_to_vote": { "voter": "inj1..." } }
//! ```

use cosmwasm_schema::{cw_serde, QueryResponses};

use crate::state::{Proposal, Voter};

/// Runs once, when the contract is deployed.
///
/// Solidity: `constructor(bytes32[] memory proposalNames)`.
/// Whoever sends this transaction becomes the chairperson and gets weight 1.
#[cw_serde]
pub struct InstantiateMsg {
    /// The fixed list of options. Order matters — you vote by index, so the
    /// first name here is proposal `0`.
    pub proposals: Vec<String>,
}

/// State-changing calls. These cost gas and need a signed transaction.
#[cw_serde]
pub enum ExecuteMsg {
    /// Chairperson only. Grants `voter` a weight of 1.
    GiveRightToVote { voter: String },

    /// Cast your weight for the proposal at this index. Final — you cannot
    /// vote twice, change your vote, or delegate afterwards.
    Vote { proposal: u32 },

    /// Hand your weight to another address. Also final. If that address has
    /// itself delegated, your weight follows the chain to the end of it.
    Delegate { to: String },
}

/// Read-only calls. Free — no gas, no transaction, no signature.
///
/// `#[derive(QueryResponses)]` plus each `#[returns(...)]` is what lets
/// `cargo schema` document the response type for every query.
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    /// Who is allowed to hand out voting rights.
    #[returns(ChairpersonResponse)]
    Chairperson {},

    /// The full ballot with live vote counts.
    #[returns(ProposalsResponse)]
    Proposals {},

    /// One address's record: weight, whether they voted, who they delegated to.
    #[returns(VoterResponse)]
    Voter { address: String },

    /// Solidity's `winningProposal()` — the index of the leading proposal.
    #[returns(WinningProposalResponse)]
    WinningProposal {},

    /// Solidity's `winnerName()` — the name of the leading proposal.
    #[returns(WinnerNameResponse)]
    WinnerName {},
}

#[cw_serde]
pub struct ChairpersonResponse {
    pub chairperson: String,
}

#[cw_serde]
pub struct ProposalsResponse {
    pub proposals: Vec<Proposal>,
}

#[cw_serde]
pub struct VoterResponse {
    /// `None` if this address was never granted the right to vote.
    pub voter: Option<Voter>,
}

#[cw_serde]
pub struct WinningProposalResponse {
    pub index: u32,
    pub vote_count: u64,
    /// True when another proposal has the same count. The index above is then
    /// just the first of the tied proposals — decide off-chain what to do.
    pub tied: bool,
}

#[cw_serde]
pub struct WinnerNameResponse {
    pub name: String,
    pub vote_count: u64,
    pub tied: bool,
}
