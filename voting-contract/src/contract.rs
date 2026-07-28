//! # The contract itself
//!
//! ## The three entry points
//!
//! Every CosmWasm contract is just three functions:
//!
//! - `instantiate` — runs once at deploy. Solidity's `constructor`.
//! - `execute` — runs for state-changing calls. Costs gas.
//! - `query` — runs for reads. Free, and *cannot* write (note it takes `Deps`,
//!   not `DepsMut` — the compiler enforces read-only at compile time).
//!
//! ## The arguments they all receive
//!
//! - `deps` — your handle to the outside world: `deps.storage` (read/write
//!   state), `deps.api` (address validation), `deps.querier` (ask the chain or
//!   other contracts things).
//! - `env` — facts about *now*: `env.block.height`, `env.block.time`,
//!   `env.contract.address`. Solidity's `block.*`.
//! - `info` — facts about *this call*: `info.sender` (Solidity's `msg.sender`)
//!   and `info.funds` (Solidity's `msg.value`, but a list of coins). Queries
//!   have no `info` — nobody signs a free read.
//!
//! ## Errors are reverts
//!
//! Returning `Err(...)` from `execute` rolls the entire transaction back —
//! nothing is written. So you can mutate freely and bail out late; there is no
//! half-applied state. This is the same guarantee Solidity's `revert` gives.

#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{to_json_binary, Binary, Deps, DepsMut, Env, MessageInfo, Response, StdResult};

use crate::error::ContractError;
use crate::msg::{
    ChairpersonResponse, ExecuteMsg, InstantiateMsg, ProposalsResponse, QueryMsg, VoterResponse,
    WinnerNameResponse, WinningProposalResponse,
};
use crate::state::{Proposal, Voter, CHAIRPERSON, PROPOSALS, VOTERS};

/// Stored on-chain so tooling can tell what code is running at an address.
/// There is no Solidity equivalent; it is a CosmWasm convention (the `cw2`
/// standard) and it is what makes safe contract migrations possible later.
const CONTRACT_NAME: &str = concat!("crates.io:", env!("CARGO_PKG_NAME"));
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Reading the ballot costs gas proportional to its length, so we cap it.
const MAX_PROPOSALS: usize = 50;

/// Delegation follows a chain (A -> B -> C). We refuse to walk it forever.
/// Solidity's original relies on running out of gas; being explicit is clearer
/// and gives the caller a real error message.
const MAX_DELEGATION_DEPTH: u32 = 20;

// ---------------------------------------------------------------------------
// instantiate  —  Solidity: constructor(bytes32[] proposalNames)
// ---------------------------------------------------------------------------

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    cw2::set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    // Validate the ballot before writing anything.
    if msg.proposals.is_empty() {
        return Err(ContractError::NoProposals {});
    }
    if msg.proposals.len() > MAX_PROPOSALS {
        return Err(ContractError::TooManyProposals {
            count: msg.proposals.len(),
            max: MAX_PROPOSALS,
        });
    }
    // Duplicate names would make the result ambiguous ("Alice" won — which one?).
    // The list is capped at 50, so this O(n^2) check is cheap.
    for (i, name) in msg.proposals.iter().enumerate() {
        if msg.proposals[..i].contains(name) {
            return Err(ContractError::DuplicateProposal { name: name.clone() });
        }
    }

    // Whoever deploys the contract is the chairperson and can already vote.
    // Solidity: `chairperson = msg.sender; voters[chairperson].weight = 1;`
    let chairperson = info.sender;
    CHAIRPERSON.save(deps.storage, &chairperson)?;
    VOTERS.save(
        deps.storage,
        &chairperson,
        &Voter {
            weight: 1,
            ..Default::default()
        },
    )?;

    let proposals: Vec<Proposal> = msg
        .proposals
        .into_iter()
        .map(|name| Proposal {
            name,
            vote_count: 0,
        })
        .collect();
    PROPOSALS.save(deps.storage, &proposals)?;

    // Attributes end up in the transaction log, where block explorers and
    // frontends can read them. They are the rough equivalent of Solidity events.
    Ok(Response::new()
        .add_attribute("action", "instantiate")
        .add_attribute("chairperson", chairperson)
        .add_attribute("proposal_count", proposals.len().to_string()))
}

// ---------------------------------------------------------------------------
// execute  —  the router
// ---------------------------------------------------------------------------

/// Deserialises the incoming JSON into `ExecuteMsg` and dispatches. `match` is
/// exhaustive, so adding a new variant to `ExecuteMsg` without handling it here
/// is a compile error rather than a runtime surprise.
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::GiveRightToVote { voter } => execute_give_right_to_vote(deps, info, voter),
        ExecuteMsg::Vote { proposal } => execute_vote(deps, info, proposal),
        ExecuteMsg::Delegate { to } => execute_delegate(deps, info, to),
    }
}

// ---------------------------------------------------------------------------
// giveRightToVote  —  chairperson only
// ---------------------------------------------------------------------------

fn execute_give_right_to_vote(
    deps: DepsMut,
    info: MessageInfo,
    voter: String,
) -> Result<Response, ContractError> {
    // Access control. Solidity: `require(msg.sender == chairperson, ...)`.
    let chairperson = CHAIRPERSON.load(deps.storage)?;
    if info.sender != chairperson {
        return Err(ContractError::NotChairperson {});
    }

    // Addresses arrive as plain strings. `addr_validate` checks the bech32
    // encoding and prefix (`inj1...` on Injective) and hands back an `Addr`.
    // Never skip this: an unvalidated string could be a typo you can never
    // undo, or an address for a different chain entirely.
    let voter_addr = deps.api.addr_validate(&voter)?;

    // An address we have never seen has no record at all, so treat "missing"
    // as "a fresh voter with weight 0". This is the explicit version of what
    // Solidity does implicitly when it returns a zeroed struct.
    let existing = VOTERS
        .may_load(deps.storage, &voter_addr)?
        .unwrap_or_default();

    if existing.voted {
        return Err(ContractError::AlreadyVoted { address: voter });
    }
    // Re-granting would silently reset a delegated-to voter's accumulated
    // weight back to 1, quietly destroying votes. Refuse instead.
    if existing.weight != 0 {
        return Err(ContractError::AlreadyHasRightToVote { address: voter });
    }

    VOTERS.save(
        deps.storage,
        &voter_addr,
        &Voter {
            weight: 1,
            ..Default::default()
        },
    )?;

    Ok(Response::new()
        .add_attribute("action", "give_right_to_vote")
        .add_attribute("voter", voter_addr))
}

// ---------------------------------------------------------------------------
// vote
// ---------------------------------------------------------------------------

fn execute_vote(
    deps: DepsMut,
    info: MessageInfo,
    proposal: u32,
) -> Result<Response, ContractError> {
    let mut voter = VOTERS
        .may_load(deps.storage, &info.sender)?
        .unwrap_or_default();

    if voter.weight == 0 {
        return Err(ContractError::NoRightToVote {
            address: info.sender.into(),
        });
    }
    if voter.voted {
        return Err(ContractError::AlreadyVoted {
            address: info.sender.into(),
        });
    }

    let mut proposals = PROPOSALS.load(deps.storage)?;
    let count = proposals.len();
    let entry = proposals
        .get_mut(proposal as usize)
        .ok_or(ContractError::ProposalNotFound {
            index: proposal,
            count,
        })?;

    // `checked_add` returns None instead of wrapping around on overflow.
    // Wrapping arithmetic is how a lot of real contracts have lost money;
    // here it is impossible for the count to silently reset to zero.
    entry.vote_count = entry
        .vote_count
        .checked_add(voter.weight)
        .ok_or(ContractError::Overflow {})?;

    voter.voted = true;
    voter.vote = Some(proposal);

    // Nothing is persisted until these `save` calls run — and if we had
    // returned an error above, none of it would have been written at all.
    PROPOSALS.save(deps.storage, &proposals)?;
    VOTERS.save(deps.storage, &info.sender, &voter)?;

    Ok(Response::new()
        .add_attribute("action", "vote")
        .add_attribute("voter", info.sender)
        .add_attribute("proposal", proposal.to_string()))
}

// ---------------------------------------------------------------------------
// delegate  —  the interesting one
// ---------------------------------------------------------------------------

/// Give your weight to someone else.
///
/// The subtle part is that delegation chains collapse. If Bob already
/// delegated to Carol, then Alice delegating to Bob really means Alice
/// delegating to Carol — otherwise Alice's weight would sit on Bob's record
/// forever, since Bob has already used his turn.
fn execute_delegate(
    deps: DepsMut,
    info: MessageInfo,
    to: String,
) -> Result<Response, ContractError> {
    let sender = info.sender;
    let mut sender_voter = VOTERS.may_load(deps.storage, &sender)?.unwrap_or_default();

    if sender_voter.weight == 0 {
        return Err(ContractError::NoRightToVote {
            address: sender.into(),
        });
    }
    if sender_voter.voted {
        return Err(ContractError::AlreadyVoted {
            address: sender.into(),
        });
    }

    let mut target = deps.api.addr_validate(&to)?;
    if target == sender {
        return Err(ContractError::SelfDelegation {});
    }

    // Walk to the end of the delegation chain.
    let mut depth = 0u32;
    while let Some(next) = VOTERS
        .may_load(deps.storage, &target)?
        .and_then(|v| v.delegate)
    {
        target = next;

        // A -> B -> A would otherwise spin forever. Solidity's version has the
        // identical check; it exists because delegation is a linked list that
        // users control, and users can point it back at themselves.
        if target == sender {
            return Err(ContractError::DelegationLoop {});
        }
        depth += 1;
        if depth > MAX_DELEGATION_DEPTH {
            return Err(ContractError::DelegationChainTooLong {
                max: MAX_DELEGATION_DEPTH,
            });
        }
    }

    let mut target_voter = VOTERS.may_load(deps.storage, &target)?.unwrap_or_default();
    if target_voter.weight == 0 {
        return Err(ContractError::DelegateHasNoRightToVote {});
    }

    // The sender is done either way: their turn is spent.
    sender_voter.voted = true;
    sender_voter.delegate = Some(target.clone());
    let transferred = sender_voter.weight;
    VOTERS.save(deps.storage, &sender, &sender_voter)?;

    if target_voter.voted {
        // They already cast their vote, so the weight cannot sit on their
        // record — push it straight onto the proposal they chose.
        //
        // `vote` is always `Some` here: the only ways to set `voted = true`
        // are voting (which sets `vote`) or delegating (which sets `delegate`,
        // and the loop above already followed every `delegate` to its end).
        // `unwrap_or_default` keeps this branch panic-free regardless.
        let index = target_voter.vote.unwrap_or_default() as usize;
        let mut proposals = PROPOSALS.load(deps.storage)?;
        let count = proposals.len();
        let entry = proposals
            .get_mut(index)
            .ok_or(ContractError::ProposalNotFound {
                index: index as u32,
                count,
            })?;
        entry.vote_count = entry
            .vote_count
            .checked_add(transferred)
            .ok_or(ContractError::Overflow {})?;
        PROPOSALS.save(deps.storage, &proposals)?;
    } else {
        // They have not voted yet, so they carry the extra weight into
        // whatever they eventually choose.
        target_voter.weight = target_voter
            .weight
            .checked_add(transferred)
            .ok_or(ContractError::Overflow {})?;
        VOTERS.save(deps.storage, &target, &target_voter)?;
    }

    Ok(Response::new()
        .add_attribute("action", "delegate")
        .add_attribute("from", sender)
        .add_attribute("to", target)
        .add_attribute("weight", transferred.to_string()))
}

// ---------------------------------------------------------------------------
// query  —  free, read-only
// ---------------------------------------------------------------------------

/// Note the return type: `StdResult<Binary>`, not `Response`. Queries hand back
/// raw JSON bytes rather than a transaction result, which is why every branch
/// ends in `to_json_binary`.
#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Chairperson {} => to_json_binary(&ChairpersonResponse {
            chairperson: CHAIRPERSON.load(deps.storage)?.into(),
        }),
        QueryMsg::Proposals {} => to_json_binary(&ProposalsResponse {
            proposals: PROPOSALS.load(deps.storage)?,
        }),
        QueryMsg::Voter { address } => to_json_binary(&query_voter(deps, address)?),
        QueryMsg::WinningProposal {} => to_json_binary(&query_winning_proposal(deps)?),
        QueryMsg::WinnerName {} => to_json_binary(&query_winner_name(deps)?),
    }
}

fn query_voter(deps: Deps, address: String) -> StdResult<VoterResponse> {
    let addr = deps.api.addr_validate(&address)?;
    Ok(VoterResponse {
        voter: VOTERS.may_load(deps.storage, &addr)?,
    })
}

/// Solidity: `winningProposal()`.
///
/// Ties: we report the *lowest* index among the leaders and set `tied: true`,
/// so callers can tell "proposal 0 won" apart from "proposal 0 and 2 are level
/// and someone must break the tie". The Solidity original silently returns the
/// first leader with no indication a tie happened at all.
fn query_winning_proposal(deps: Deps) -> StdResult<WinningProposalResponse> {
    let proposals = PROPOSALS.load(deps.storage)?;

    // `instantiate` rejects an empty ballot, so there is always a proposal 0.
    let best = proposals.iter().map(|p| p.vote_count).max().unwrap_or(0);
    let index = proposals
        .iter()
        .position(|p| p.vote_count == best)
        .unwrap_or(0);
    let tied = proposals.iter().filter(|p| p.vote_count == best).count() > 1;

    Ok(WinningProposalResponse {
        index: index as u32,
        vote_count: best,
        tied,
    })
}

/// Solidity: `winnerName()`.
fn query_winner_name(deps: Deps) -> StdResult<WinnerNameResponse> {
    let winner = query_winning_proposal(deps)?;
    let proposals = PROPOSALS.load(deps.storage)?;
    Ok(WinnerNameResponse {
        name: proposals[winner.index as usize].name.clone(),
        vote_count: winner.vote_count,
        tied: winner.tied,
    })
}
