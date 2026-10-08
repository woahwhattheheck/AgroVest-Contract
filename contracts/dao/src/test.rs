use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::token::{StellarAssetClient, TokenClient};
use soroban_sdk::{contract, contractimpl, Address, Env, String};

use crate::types::VoteData;
use crate::{DaoContract, DaoContractClient};

#[contract]
pub struct MockInvestment;

#[contractimpl]
impl MockInvestment {
    pub fn create_investment(
        _env: Env,
        _farm_id: u32,
        _image: String,
        _name: String,
        _about: String,
        _min_amount: i128,
        _end_date: u64,
        _owner: Address,
    ) {
    }
}

struct TestCtx<'a> {
    env: Env,
    token_addr: Address,
    contract_addr: Address,
    admin: Address,
    client: DaoContractClient<'a>,
}

fn setup<'a>() -> TestCtx<'a> {
    let env = Env::default();
    env.mock_all_auths();

    let token_admin = Address::generate(&env);
    let token_addr = env.register_stellar_asset_contract(token_admin.clone());

    let admin = Address::generate(&env);
    let investment_addr = env.register_contract(None, MockInvestment);
    let contract_addr = env.register_contract(None, DaoContract);
    let client = DaoContractClient::new(&env, &contract_addr);

    client.initialize(&admin, &token_addr, &investment_addr);

    TestCtx {
        env,
        token_addr,
        contract_addr,
        admin,
        client,
    }
}

fn mint(ctx: &TestCtx, to: &Address, amount: i128) {
    let sac = StellarAssetClient::new(&ctx.env, &ctx.token_addr);
    sac.mock_all_auths().mint(to, &amount);
}

fn approve(ctx: &TestCtx, from: &Address, amount: i128) {
    let token = TokenClient::new(&ctx.env, &ctx.token_addr);
    let max_ttl = ctx.env.ledger().sequence() + 100;
    token
        .mock_all_auths()
        .approve(from, &ctx.contract_addr, &amount, &max_ttl);
}

#[test]
fn test_initialize() {
    let _ctx = setup();
}

#[test]
fn test_lock_and_unlock_tokens() {
    let ctx = setup();
    let user = Address::generate(&ctx.env);

    mint(&ctx, &user, 1000);
    approve(&ctx, &user, 1000);

    ctx.client.lock_tokens(&user, &1000i128);

    let balance = ctx.client.get_token_balance(&user);
    assert_eq!(balance, 1000);

    ctx.client.unlock_tokens(&user);

    let balance = ctx.client.get_token_balance(&user);
    assert_eq!(balance, 0);
}

#[test]
fn test_lock_tokens_accumulates() {
    let ctx = setup();
    let user = Address::generate(&ctx.env);

    mint(&ctx, &user, 1000);
    approve(&ctx, &user, 1000);

    ctx.client.lock_tokens(&user, &100i128);
    assert_eq!(ctx.client.get_token_balance(&user), 100);

    ctx.client.lock_tokens(&user, &50i128);
    assert_eq!(ctx.client.get_token_balance(&user), 150);
}

#[test]
fn test_create_proposal() {
    let ctx = setup();
    let proposer = Address::generate(&ctx.env);
    let title = String::from_str(&ctx.env, "Fund Rice Farm");
    let desc = String::from_str(&ctx.env, "Proposal to fund rice farming operations");
    let ends_at = ctx.env.ledger().timestamp() + 86400;

    ctx.client
        .create_proposal(&proposer, &title, &desc, &100i128, &ends_at);

    let proposal = ctx.client.get_proposal(&1u32);
    assert_eq!(proposal.title, title);
    assert_eq!(proposal.required_votes, 100);
}

#[test]
fn test_vote_proposal() {
    let ctx = setup();
    let proposer = Address::generate(&ctx.env);
    let voter = Address::generate(&ctx.env);
    let title = String::from_str(&ctx.env, "Fund Rice Farm");
    let desc = String::from_str(&ctx.env, "Proposal to fund rice farming operations");
    let ends_at = ctx.env.ledger().timestamp() + 86400;

    mint(&ctx, &voter, 1000);
    approve(&ctx, &voter, 1000);
    ctx.client.lock_tokens(&voter, &1000i128);

    ctx.client
        .create_proposal(&proposer, &title, &desc, &100i128, &ends_at);
    ctx.client.vote_proposal(&voter, &1u32, &VoteData::Accept);

    let proposal = ctx.client.get_proposal(&1u32);
    assert!(proposal.accept_votes > 0);
}

#[test]
fn test_delegate() {
    let ctx = setup();
    let delegator = Address::generate(&ctx.env);
    let delegatee = Address::generate(&ctx.env);

    ctx.client.delegate(&delegator, &delegatee);

    let d = ctx.client.get_delegate(&delegator);
    assert_eq!(d, delegatee);
}

#[test]
fn test_create_challenge() {
    let ctx = setup();
    let proposer = Address::generate(&ctx.env);
    let challenger = Address::generate(&ctx.env);
    let title = String::from_str(&ctx.env, "Fund Rice Farm");
    let desc = String::from_str(&ctx.env, "Proposal to fund rice farming operations");
    let ends_at = ctx.env.ledger().timestamp() + 86400;

    ctx.client
        .create_proposal(&proposer, &title, &desc, &100i128, &ends_at);

    let chall_desc = String::from_str(&ctx.env, "This proposal is invalid");
    ctx.client.create_challenge(&challenger, &1u32, &chall_desc);

    let challenge = ctx.client.get_challenge(&1u32);
    assert_eq!(challenge.proposal_id, 1);
    assert!(!challenge.resolved);
}

#[test]
fn test_dispute_flow() {
    let ctx = setup();
    let proposer = Address::generate(&ctx.env);
    let challenger = Address::generate(&ctx.env);
    let arbitrator = Address::generate(&ctx.env);
    let title = String::from_str(&ctx.env, "Fund Rice Farm");
    let desc = String::from_str(&ctx.env, "Proposal to fund rice farming operations");
    let ends_at = ctx.env.ledger().timestamp() + 86400;

    ctx.client
        .create_proposal(&proposer, &title, &desc, &100i128, &ends_at);

    let chall_desc = String::from_str(&ctx.env, "Invalid proposal");
    ctx.client.create_challenge(&challenger, &1u32, &chall_desc);
    ctx.client.initiate_dispute(&challenger, &1u32, &arbitrator);

    let dispute = ctx.client.get_dispute(&1u32);
    assert_eq!(dispute.arbitrator, arbitrator);
    assert!(!dispute.resolved);

    ctx.client.resolve_dispute(&arbitrator, &1u32, &true);

    let dispute = ctx.client.get_dispute(&1u32);
    assert!(dispute.resolved);
    assert!(dispute.ruling);
}

#[test]
#[should_panic(expected = "ProposalNotEnded")]
fn test_tally_votes_before_proposal_end_fails() {
    let ctx = setup();
    let proposer = Address::generate(&ctx.env);
    let caller = Address::generate(&ctx.env);
    let title = String::from_str(&ctx.env, "Fund Rice Farm");
    let desc = String::from_str(&ctx.env, "Proposal to fund rice farming operations");
    let ends_at = ctx.env.ledger().timestamp() + 86400;

    ctx.client
        .create_proposal(&proposer, &title, &desc, &100i128, &ends_at);

    ctx.client.tally_votes(&caller, &1u32);
}

#[test]
#[should_panic(expected = "InsufficientVotes")]
fn test_execute_proposal_reject_votes_fails() {
    let ctx = setup();
    let proposer = Address::generate(&ctx.env);
    let voter = Address::generate(&ctx.env);
    let title = String::from_str(&ctx.env, "Fund Rice Farm");
    let desc = String::from_str(&ctx.env, "Proposal to fund rice farming operations");
    let ends_at = ctx.env.ledger().timestamp() + 86400;

    mint(&ctx, &voter, 1000);
    approve(&ctx, &voter, 1000);
    ctx.client.lock_tokens(&voter, &1000i128);

    ctx.client
        .create_proposal(&proposer, &title, &desc, &100i128, &ends_at);
    ctx.client.vote_proposal(&voter, &1u32, &VoteData::Reject);
    ctx.env.ledger().set_timestamp(ends_at);
    ctx.client.tally_votes(&voter, &1u32);

    let name = String::from_str(&ctx.env, "Rice Farm");
    let about = String::from_str(&ctx.env, "A rice farm");
    ctx.client
        .execute_proposal(&proposer, &1u32, &1u32, &name, &about, &100i128, &ends_at);
}

#[test]
#[should_panic(expected = "InsufficientVotes")]
fn test_execute_proposal_insufficient_accept_votes_fails() {
    let ctx = setup();
    let proposer = Address::generate(&ctx.env);
    let voter = Address::generate(&ctx.env);
    let title = String::from_str(&ctx.env, "Fund Rice Farm");
    let desc = String::from_str(&ctx.env, "Proposal to fund rice farming operations");
    let ends_at = ctx.env.ledger().timestamp() + 86400;

    mint(&ctx, &voter, 100);
    approve(&ctx, &voter, 100);
    ctx.client.lock_tokens(&voter, &100i128);

    ctx.client
        .create_proposal(&proposer, &title, &desc, &1000i128, &ends_at);
    ctx.client.vote_proposal(&voter, &1u32, &VoteData::Accept);
    ctx.env.ledger().set_timestamp(ends_at);
    ctx.client.tally_votes(&voter, &1u32);

    let proposal = ctx.client.get_proposal(&1u32);
    assert!(proposal.accept_votes < proposal.required_votes);

    let name = String::from_str(&ctx.env, "Rice Farm");
    let about = String::from_str(&ctx.env, "A rice farm");
    ctx.client
        .execute_proposal(&proposer, &1u32, &1u32, &name, &about, &100i128, &ends_at);
}

#[test]
fn test_execute_proposal_sufficient_accept_votes_succeeds() {
    let ctx = setup();
    let proposer = Address::generate(&ctx.env);
    let voter = Address::generate(&ctx.env);
    let title = String::from_str(&ctx.env, "Fund Rice Farm");
    let desc = String::from_str(&ctx.env, "Proposal to fund rice farming operations");
    let ends_at = ctx.env.ledger().timestamp() + 86400;

    mint(&ctx, &voter, 10000);
    approve(&ctx, &voter, 10000);
    ctx.client.lock_tokens(&voter, &10000i128);

    ctx.client
        .create_proposal(&proposer, &title, &desc, &50i128, &ends_at);
    ctx.client.vote_proposal(&voter, &1u32, &VoteData::Accept);
    ctx.env.ledger().set_timestamp(ends_at);
    ctx.client.tally_votes(&voter, &1u32);

    let proposal = ctx.client.get_proposal(&1u32);
    assert!(proposal.accept_votes >= proposal.required_votes);

    let name = String::from_str(&ctx.env, "Rice Farm");
    let about = String::from_str(&ctx.env, "A rice farm");
    ctx.client
        .execute_proposal(&proposer, &1u32, &1u32, &name, &about, &100i128, &ends_at);

    let proposal = ctx.client.get_proposal(&1u32);
    assert!(proposal.executed);
}

#[test]
#[should_panic(expected = "NotAdmin")]
fn test_resolve_challenge_non_admin_panics() {
    let ctx = setup();
    let proposer = Address::generate(&ctx.env);
    let challenger = Address::generate(&ctx.env);
    let non_admin = Address::generate(&ctx.env);
    let title = String::from_str(&ctx.env, "Fund Rice Farm");
    let desc = String::from_str(&ctx.env, "Proposal to fund rice farming operations");
    let ends_at = ctx.env.ledger().timestamp() + 86400;

    ctx.client
        .create_proposal(&proposer, &title, &desc, &100i128, &ends_at);

    let chall_desc = String::from_str(&ctx.env, "This proposal is invalid");
    ctx.client.create_challenge(&challenger, &1u32, &chall_desc);

    ctx.client.resolve_challenge(&non_admin, &1u32, &true);
}

#[test]
fn test_resolve_challenge_admin_succeeds() {
    let ctx = setup();
    let proposer = Address::generate(&ctx.env);
    let challenger = Address::generate(&ctx.env);
    let title = String::from_str(&ctx.env, "Fund Rice Farm");
    let desc = String::from_str(&ctx.env, "Proposal to fund rice farming operations");
    let ends_at = ctx.env.ledger().timestamp() + 86400;

    ctx.client
        .create_proposal(&proposer, &title, &desc, &100i128, &ends_at);

    let chall_desc = String::from_str(&ctx.env, "This proposal is invalid");
    ctx.client.create_challenge(&challenger, &1u32, &chall_desc);

    ctx.client.resolve_challenge(&ctx.admin, &1u32, &true);

    let challenge = ctx.client.get_challenge(&1u32);
    assert!(challenge.resolved);
    assert!(challenge.valid);
}
