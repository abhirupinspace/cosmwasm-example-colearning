//! # Tests
//!
//! These use `cw-multi-test`, which simulates a blockchain in memory. There is
//! no node, no Docker and no network — `cargo test` runs the real contract code
//! against a real storage backend in about a second.
//!
//! Read these top to bottom and you have a full description of how the ballot
//! behaves, including every rule it refuses to break.

use cosmwasm_std::{testing::MockApi, Addr};
use cw_multi_test::{App, AppBuilder, BankKeeper, Contract, ContractWrapper, Executor};

use voting_contract::contract::{execute, instantiate, query};
use voting_contract::msg::{
    ChairpersonResponse, ExecuteMsg, InstantiateMsg, ProposalsResponse, QueryMsg, VoterResponse,
    WinnerNameResponse, WinningProposalResponse,
};
use voting_contract::ContractError;

/// A blockchain that uses Injective's `inj` bech32 prefix, so the addresses in
/// these tests look like the ones you will use on testnet.
type TestApp = App<BankKeeper, MockApi>;

/// Bundles the contract's three entry points so the test chain can run them.
fn contract() -> Box<dyn Contract<cosmwasm_std::Empty>> {
    Box::new(ContractWrapper::new(execute, instantiate, query))
}

/// Boots a chain and deploys a 3-option ballot.
/// Returns the chain, the contract address, and four funded-looking addresses.
fn setup() -> (TestApp, Addr, Addr, Addr, Addr, Addr) {
    let mut app = AppBuilder::new()
        .with_api(MockApi::default().with_prefix("inj"))
        .build(|_, _, _| {});

    // `addr_make` derives a valid bech32 address from a label — the test
    // equivalent of a wallet. No private keys involved.
    let chair = app.api().addr_make("chairperson");
    let alice = app.api().addr_make("alice");
    let bob = app.api().addr_make("bob");
    let carol = app.api().addr_make("carol");

    let code_id = app.store_code(contract());
    let addr = app
        .instantiate_contract(
            code_id,
            chair.clone(),
            &InstantiateMsg {
                proposals: vec!["Alpha".into(), "Beta".into(), "Gamma".into()],
            },
            &[],
            "ballot",
            None,
        )
        .unwrap();

    (app, addr, chair, alice, bob, carol)
}

// --- small helpers so the tests below read like sentences -------------------

fn grant(app: &mut TestApp, ballot: &Addr, chair: &Addr, voter: &Addr) {
    app.execute_contract(
        chair.clone(),
        ballot.clone(),
        &ExecuteMsg::GiveRightToVote {
            voter: voter.to_string(),
        },
        &[],
    )
    .unwrap();
}

fn vote(app: &mut TestApp, ballot: &Addr, who: &Addr, proposal: u32) {
    app.execute_contract(
        who.clone(),
        ballot.clone(),
        &ExecuteMsg::Vote { proposal },
        &[],
    )
    .unwrap();
}

fn delegate(app: &mut TestApp, ballot: &Addr, from: &Addr, to: &Addr) {
    app.execute_contract(
        from.clone(),
        ballot.clone(),
        &ExecuteMsg::Delegate { to: to.to_string() },
        &[],
    )
    .unwrap();
}

fn counts(app: &TestApp, ballot: &Addr) -> Vec<u64> {
    let res: ProposalsResponse = app
        .wrap()
        .query_wasm_smart(ballot, &QueryMsg::Proposals {})
        .unwrap();
    res.proposals.iter().map(|p| p.vote_count).collect()
}

fn weight_of(app: &TestApp, ballot: &Addr, who: &Addr) -> u64 {
    let res: VoterResponse = app
        .wrap()
        .query_wasm_smart(
            ballot,
            &QueryMsg::Voter {
                address: who.to_string(),
            },
        )
        .unwrap();
    res.voter.map(|v| v.weight).unwrap_or(0)
}

fn winner(app: &TestApp, ballot: &Addr) -> WinnerNameResponse {
    app.wrap()
        .query_wasm_smart(ballot, &QueryMsg::WinnerName {})
        .unwrap()
}

// --- instantiate ------------------------------------------------------------

#[test]
fn deployer_becomes_chairperson_and_can_vote() {
    let (app, ballot, chair, ..) = setup();

    let res: ChairpersonResponse = app
        .wrap()
        .query_wasm_smart(&ballot, &QueryMsg::Chairperson {})
        .unwrap();
    assert_eq!(res.chairperson, chair.to_string());

    // The chairperson starts with weight 1, exactly like the Solidity original.
    assert_eq!(weight_of(&app, &ballot, &chair), 1);
    assert_eq!(counts(&app, &ballot), vec![0, 0, 0]);
}

#[test]
fn empty_ballot_is_rejected() {
    let mut app = AppBuilder::new()
        .with_api(MockApi::default().with_prefix("inj"))
        .build(|_, _, _| {});
    let chair = app.api().addr_make("chairperson");
    let code_id = app.store_code(contract());

    let err: ContractError = app
        .instantiate_contract(
            code_id,
            chair,
            &InstantiateMsg { proposals: vec![] },
            &[],
            "ballot",
            None,
        )
        .unwrap_err()
        .downcast()
        .unwrap();
    assert_eq!(err, ContractError::NoProposals {});
}

#[test]
fn duplicate_proposal_names_are_rejected() {
    let mut app = AppBuilder::new()
        .with_api(MockApi::default().with_prefix("inj"))
        .build(|_, _, _| {});
    let chair = app.api().addr_make("chairperson");
    let code_id = app.store_code(contract());

    let err: ContractError = app
        .instantiate_contract(
            code_id,
            chair,
            &InstantiateMsg {
                proposals: vec!["Alpha".into(), "Alpha".into()],
            },
            &[],
            "ballot",
            None,
        )
        .unwrap_err()
        .downcast()
        .unwrap();
    assert_eq!(
        err,
        ContractError::DuplicateProposal {
            name: "Alpha".into()
        }
    );
}

// --- giveRightToVote --------------------------------------------------------

#[test]
fn chairperson_grants_voting_rights() {
    let (mut app, ballot, chair, alice, ..) = setup();

    assert_eq!(weight_of(&app, &ballot, &alice), 0);
    grant(&mut app, &ballot, &chair, &alice);
    assert_eq!(weight_of(&app, &ballot, &alice), 1);
}

#[test]
fn non_chairperson_cannot_grant_voting_rights() {
    let (mut app, ballot, _chair, alice, bob, _) = setup();

    let err: ContractError = app
        .execute_contract(
            alice,
            ballot,
            &ExecuteMsg::GiveRightToVote {
                voter: bob.to_string(),
            },
            &[],
        )
        .unwrap_err()
        .downcast()
        .unwrap();
    assert_eq!(err, ContractError::NotChairperson {});
}

#[test]
fn granting_twice_is_rejected() {
    let (mut app, ballot, chair, alice, ..) = setup();
    grant(&mut app, &ballot, &chair, &alice);

    // This matters: a second grant would reset weight to 1 and silently destroy
    // any weight that had been delegated to Alice in the meantime.
    let err: ContractError = app
        .execute_contract(
            chair,
            ballot,
            &ExecuteMsg::GiveRightToVote {
                voter: alice.to_string(),
            },
            &[],
        )
        .unwrap_err()
        .downcast()
        .unwrap();
    assert_eq!(
        err,
        ContractError::AlreadyHasRightToVote {
            address: alice.to_string()
        }
    );
}

// --- vote -------------------------------------------------------------------

#[test]
fn voting_adds_weight_to_the_chosen_proposal() {
    let (mut app, ballot, chair, alice, bob, _) = setup();
    grant(&mut app, &ballot, &chair, &alice);
    grant(&mut app, &ballot, &chair, &bob);

    vote(&mut app, &ballot, &alice, 0);
    vote(&mut app, &ballot, &bob, 2);
    vote(&mut app, &ballot, &chair, 0);

    assert_eq!(counts(&app, &ballot), vec![2, 0, 1]);
    assert_eq!(winner(&app, &ballot).name, "Alpha");
}

#[test]
fn a_stranger_cannot_vote() {
    let (mut app, ballot, _chair, alice, ..) = setup();

    let err: ContractError = app
        .execute_contract(
            alice.clone(),
            ballot,
            &ExecuteMsg::Vote { proposal: 0 },
            &[],
        )
        .unwrap_err()
        .downcast()
        .unwrap();
    assert_eq!(
        err,
        ContractError::NoRightToVote {
            address: alice.to_string()
        }
    );
}

#[test]
fn voting_twice_is_rejected() {
    let (mut app, ballot, chair, alice, ..) = setup();
    grant(&mut app, &ballot, &chair, &alice);
    vote(&mut app, &ballot, &alice, 0);

    let err: ContractError = app
        .execute_contract(
            alice.clone(),
            ballot.clone(),
            &ExecuteMsg::Vote { proposal: 1 },
            &[],
        )
        .unwrap_err()
        .downcast()
        .unwrap();
    assert_eq!(
        err,
        ContractError::AlreadyVoted {
            address: alice.to_string()
        }
    );
    // The failed transaction changed nothing — proposal 1 is still on zero.
    assert_eq!(counts(&app, &ballot), vec![1, 0, 0]);
}

#[test]
fn voting_for_a_nonexistent_proposal_is_rejected() {
    let (mut app, ballot, chair, alice, ..) = setup();
    grant(&mut app, &ballot, &chair, &alice);

    let err: ContractError = app
        .execute_contract(alice, ballot, &ExecuteMsg::Vote { proposal: 9 }, &[])
        .unwrap_err()
        .downcast()
        .unwrap();
    assert_eq!(err, ContractError::ProposalNotFound { index: 9, count: 3 });
}

// --- delegate ---------------------------------------------------------------

#[test]
fn delegation_moves_weight_to_someone_who_has_not_voted() {
    let (mut app, ballot, chair, alice, bob, _) = setup();
    grant(&mut app, &ballot, &chair, &alice);
    grant(&mut app, &ballot, &chair, &bob);

    delegate(&mut app, &ballot, &alice, &bob);

    // Alice is spent; Bob now carries two votes.
    assert_eq!(weight_of(&app, &ballot, &alice), 1); // her own weight is unchanged...
    assert_eq!(weight_of(&app, &ballot, &bob), 2); // ...but Bob's grew
    assert_eq!(counts(&app, &ballot), vec![0, 0, 0]); // nothing counted yet

    vote(&mut app, &ballot, &bob, 1);
    assert_eq!(counts(&app, &ballot), vec![0, 2, 0]);
}

#[test]
fn delegating_to_someone_who_already_voted_counts_immediately() {
    let (mut app, ballot, chair, alice, bob, _) = setup();
    grant(&mut app, &ballot, &chair, &alice);
    grant(&mut app, &ballot, &chair, &bob);

    vote(&mut app, &ballot, &bob, 2);
    assert_eq!(counts(&app, &ballot), vec![0, 0, 1]);

    // Bob's turn is over, so Alice's weight cannot sit on his record.
    // It goes straight onto the proposal he picked.
    delegate(&mut app, &ballot, &alice, &bob);
    assert_eq!(counts(&app, &ballot), vec![0, 0, 2]);
}

#[test]
fn delegation_chains_collapse_to_the_final_delegate() {
    let (mut app, ballot, chair, alice, bob, carol) = setup();
    grant(&mut app, &ballot, &chair, &alice);
    grant(&mut app, &ballot, &chair, &bob);
    grant(&mut app, &ballot, &chair, &carol);

    // Bob -> Carol, then Alice -> Bob. Alice's weight must land on Carol,
    // because Bob has already spent his turn.
    delegate(&mut app, &ballot, &bob, &carol);
    delegate(&mut app, &ballot, &alice, &bob);

    assert_eq!(weight_of(&app, &ballot, &carol), 3);

    vote(&mut app, &ballot, &carol, 0);
    assert_eq!(counts(&app, &ballot), vec![3, 0, 0]);
}

#[test]
fn self_delegation_is_rejected() {
    let (mut app, ballot, chair, alice, ..) = setup();
    grant(&mut app, &ballot, &chair, &alice);

    let err: ContractError = app
        .execute_contract(
            alice.clone(),
            ballot,
            &ExecuteMsg::Delegate {
                to: alice.to_string(),
            },
            &[],
        )
        .unwrap_err()
        .downcast()
        .unwrap();
    assert_eq!(err, ContractError::SelfDelegation {});
}

#[test]
fn delegation_loops_are_rejected() {
    let (mut app, ballot, chair, alice, bob, _) = setup();
    grant(&mut app, &ballot, &chair, &alice);
    grant(&mut app, &ballot, &chair, &bob);

    // Bob -> Alice. If Alice could now delegate to Bob, following the chain
    // would run in circles forever.
    delegate(&mut app, &ballot, &bob, &alice);

    let err: ContractError = app
        .execute_contract(
            alice,
            ballot,
            &ExecuteMsg::Delegate {
                to: bob.to_string(),
            },
            &[],
        )
        .unwrap_err()
        .downcast()
        .unwrap();
    assert_eq!(err, ContractError::DelegationLoop {});
}

#[test]
fn cannot_delegate_to_someone_without_voting_rights() {
    let (mut app, ballot, chair, alice, bob, _) = setup();
    grant(&mut app, &ballot, &chair, &alice); // bob is never granted anything

    let err: ContractError = app
        .execute_contract(
            alice,
            ballot,
            &ExecuteMsg::Delegate {
                to: bob.to_string(),
            },
            &[],
        )
        .unwrap_err()
        .downcast()
        .unwrap();
    assert_eq!(err, ContractError::DelegateHasNoRightToVote {});
}

#[test]
fn cannot_delegate_after_voting() {
    let (mut app, ballot, chair, alice, bob, _) = setup();
    grant(&mut app, &ballot, &chair, &alice);
    grant(&mut app, &ballot, &chair, &bob);
    vote(&mut app, &ballot, &alice, 0);

    let err: ContractError = app
        .execute_contract(
            alice.clone(),
            ballot,
            &ExecuteMsg::Delegate {
                to: bob.to_string(),
            },
            &[],
        )
        .unwrap_err()
        .downcast()
        .unwrap();
    assert_eq!(
        err,
        ContractError::AlreadyVoted {
            address: alice.to_string()
        }
    );
}

// --- winner -----------------------------------------------------------------

#[test]
fn winner_reports_index_name_and_count() {
    let (mut app, ballot, chair, alice, bob, _) = setup();
    grant(&mut app, &ballot, &chair, &alice);
    grant(&mut app, &ballot, &chair, &bob);

    vote(&mut app, &ballot, &alice, 1);
    vote(&mut app, &ballot, &bob, 1);
    vote(&mut app, &ballot, &chair, 0);

    let res: WinningProposalResponse = app
        .wrap()
        .query_wasm_smart(&ballot, &QueryMsg::WinningProposal {})
        .unwrap();
    assert_eq!((res.index, res.vote_count, res.tied), (1, 2, false));
    assert_eq!(winner(&app, &ballot).name, "Beta");
}

#[test]
fn ties_are_flagged_rather_than_hidden() {
    let (mut app, ballot, chair, alice, ..) = setup();
    grant(&mut app, &ballot, &chair, &alice);

    vote(&mut app, &ballot, &chair, 0);
    vote(&mut app, &ballot, &alice, 1);

    let res = winner(&app, &ballot);
    assert!(res.tied);
    assert_eq!(res.name, "Alpha"); // lowest index among the leaders
    assert_eq!(res.vote_count, 1);
}

#[test]
fn a_ballot_with_no_votes_is_a_tie_at_zero() {
    let (app, ballot, ..) = setup();

    let res = winner(&app, &ballot);
    assert!(res.tied);
    assert_eq!(res.vote_count, 0);
}
