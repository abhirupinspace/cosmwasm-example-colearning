//! # State — what the contract remembers between transactions
//!
//! A smart contract has no RAM that survives a transaction. Everything it needs
//! to remember must be written into the blockchain's key-value store. This file
//! declares *what* we store and *under which keys*.
//!
//! Solidity does this implicitly — `address public chairperson;` is both a
//! declaration and a storage slot. CosmWasm makes it explicit: you declare a
//! typed accessor (`Item` or `Map`) and hand it a string key.

use cosmwasm_schema::cw_serde;
use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};

/// One option people can vote for.
///
/// Solidity equivalent:
/// ```solidity
/// struct Proposal { bytes32 name; uint voteCount; }
/// ```
/// We use `String` instead of `bytes32` because CosmWasm has no 32-byte-string
/// limitation — JSON in, JSON out.
#[cw_serde]
pub struct Proposal {
    pub name: String,
    /// Sum of the weights of everyone who voted for this proposal.
    pub vote_count: u64,
}

/// Everything we know about one participant.
///
/// Solidity equivalent:
/// ```solidity
/// struct Voter { uint weight; bool voted; address delegate; uint vote; }
/// ```
///
/// Note how Rust's `Option` replaces Solidity's "zero value means unset"
/// convention. In Solidity `delegate == address(0)` means "no delegate"; here
/// it is `None`, which the compiler forces you to handle.
#[cw_serde]
#[derive(Default)]
pub struct Voter {
    /// How many votes this address carries. `0` means "not allowed to vote".
    /// Starts at 1 when the chairperson grants the right, and grows as other
    /// people delegate to this address.
    pub weight: u64,
    /// True once this address has either voted directly or delegated away.
    /// Either action is final.
    pub voted: bool,
    /// Who this address delegated to, if anyone.
    pub delegate: Option<Addr>,
    /// Index into the proposal list, set only if `voted` is true *and* the
    /// address voted directly rather than delegating.
    pub vote: Option<u32>,
}

/// The address allowed to hand out voting rights. Set once, at instantiation.
///
/// `Item<T>` stores exactly one value under one key — the CosmWasm equivalent
/// of a single Solidity state variable.
pub const CHAIRPERSON: Item<Addr> = Item::new("chairperson");

/// The ballot itself. Fixed at instantiation and never resized, so keeping the
/// whole list in one `Item` is simple and cheap to read.
pub const PROPOSALS: Item<Vec<Proposal>> = Item::new("proposals");

/// Address -> their `Voter` record.
///
/// `Map<K, V>` is the equivalent of Solidity's `mapping(address => Voter)`.
/// One important difference: a Solidity mapping silently returns a zeroed
/// struct for unknown keys, while `Map::may_load` returns `None` — so
/// "never heard of this address" is something you must handle on purpose.
pub const VOTERS: Map<&Addr, Voter> = Map::new("voters");
