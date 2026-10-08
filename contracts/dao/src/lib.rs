#![no_std]
#![allow(clippy::too_many_arguments)]

mod errors;
mod storage;
mod types;

use errors::DaoError;
use types::{ChallengeData, DisputeData, ProposalData, VoteData, Votes};

use soroban_sdk::{
    contract, contractimpl, token::TokenClient, vec, Address, Env, IntoVal, String, Symbol,
};

#[contract]
pub struct DaoContract;

#[contractimpl]
impl DaoContract {
    /// Initialize the DAO contract.
    pub fn initialize(env: Env, admin: Address, token: Address, investment: Address) {
        if env.storage().instance().has(&Symbol::new(&env, "admin")) {
            panic!("{:?}", DaoError::AlreadyInitialized);
        }
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "admin"), &admin);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "token"), &token);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "investment"), &investment);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "prop_ctr"), &0u32);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "chall_ctr"), &0u32);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "disp_ctr"), &0u32);
    }

    /// Lock tokens for voting power.
    pub fn lock_tokens(env: Env, caller: Address, amount: i128) {
        caller.require_auth();

        let locked_key = (Symbol::new(&env, "locked"), caller.clone());
        let existing: i128 = env.storage().persistent().get(&locked_key).unwrap_or(0);
        let new_amount = existing + amount;
        env.storage().persistent().set(&locked_key, &new_amount);

        let token: Address = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "token"))
            .unwrap();
        let contract_addr = env.current_contract_address();
        let token_client = TokenClient::new(&env, &token);
        token_client.transfer_from(&contract_addr, &caller, &contract_addr, &amount);

        env.events().publish(
            (Symbol::new(&env, "dao"), Symbol::new(&env, "token_locked")),
            (amount, caller),
        );
    }

    /// Unlock tokens.
    pub fn unlock_tokens(env: Env, caller: Address) {
        caller.require_auth();

        let locked_key = (Symbol::new(&env, "locked"), caller.clone());
        let amount: i128 = env.storage().persistent().get(&locked_key).unwrap_or(0);

        if amount == 0 {
            panic!("{:?}", DaoError::NoTokenLocked);
        }

        let token: Address = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "token"))
            .unwrap();
        let contract_addr = env.current_contract_address();
        let token_client = TokenClient::new(&env, &token);
        token_client.transfer(&contract_addr, &caller, &amount);
        env.storage().persistent().remove(&locked_key);

        env.events().publish(
            (
                Symbol::new(&env, "dao"),
                Symbol::new(&env, "token_unlocked"),
            ),
            (amount, caller),
        );
    }

    /// Get locked token balance.
    pub fn get_token_balance(env: Env, caller: Address) -> i128 {
        let locked_key = (Symbol::new(&env, "locked"), caller);
        env.storage().persistent().get(&locked_key).unwrap_or(0)
    }

    /// Create a new proposal.
    pub fn create_proposal(
        env: Env,
        caller: Address,
        title: String,
        description: String,
        required_votes: i128,
        ends_at: u64,
    ) {
        caller.require_auth();

        let mut prop_counter: u32 = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "prop_ctr"))
            .unwrap_or(0);
        prop_counter += 1;

        let proposal = ProposalData {
            is_challenged: false,
            proposal_id: prop_counter,
            title: title.clone(),
            description,
            created_at: env.ledger().timestamp(),
            ends_at,
            required_votes,
            proposer: caller.clone(),
            executed: false,
            accept_votes: 0,
            reject_votes: 0,
            undecided_votes: 0,
        };

        let prop_key = (Symbol::new(&env, "prop"), prop_counter);
        env.storage().persistent().set(&prop_key, &proposal);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "prop_ctr"), &prop_counter);

        env.events().publish(
            (Symbol::new(&env, "dao"), Symbol::new(&env, "new_proposal")),
            (prop_counter, title, caller),
        );
    }

    /// Get a proposal by ID.
    pub fn get_proposal(env: Env, id: u32) -> ProposalData {
        let prop_key = (Symbol::new(&env, "prop"), id);
        env.storage()
            .persistent()
            .get(&prop_key)
            .unwrap_or_else(|| panic!("{:?}", DaoError::ProposalNotFound))
    }

    /// Calculate voting power (integer square root of locked tokens).
    pub fn calculate_voting_power(env: Env, user: Address) -> i128 {
        let locked_key = (Symbol::new(&env, "locked"), user);
        let amount: i128 = env.storage().persistent().get(&locked_key).unwrap_or(0);

        if amount == 0 {
            panic!("{:?}", DaoError::NoTokenLocked);
        }

        // Integer square root
        let mut x = amount;
        let mut y = (x + 1) / 2;
        while y < x {
            x = y;
            y = (x + amount / x) / 2;
        }
        x
    }

    /// Vote on a proposal.
    pub fn vote_proposal(env: Env, caller: Address, proposal_id: u32, vote: VoteData) {
        caller.require_auth();

        let voted_key = (Symbol::new(&env, "voted"), proposal_id, caller.clone());
        if env
            .storage()
            .persistent()
            .get::<_, bool>(&voted_key)
            .unwrap_or(false)
        {
            panic!("{:?}", DaoError::AlreadyVoted);
        }

        let voting_power = Self::calculate_voting_power(env.clone(), caller.clone());

        let vote_data = Votes {
            proposal_id,
            voter: caller.clone(),
            voting_power,
            vote_type: vote,
        };

        let vote_key = (Symbol::new(&env, "vote"), proposal_id, caller.clone());
        env.storage().persistent().set(&vote_key, &vote_data);
        env.storage().persistent().set(&voted_key, &true);

        // Update proposal vote counts
        let prop_key = (Symbol::new(&env, "prop"), proposal_id);
        let mut proposal: ProposalData = env
            .storage()
            .persistent()
            .get(&prop_key)
            .unwrap_or_else(|| panic!("{:?}", DaoError::ProposalNotFound));

        match vote {
            VoteData::Accept => proposal.accept_votes += voting_power,
            VoteData::Reject => proposal.reject_votes += voting_power,
            VoteData::Undecided => proposal.undecided_votes += voting_power,
            VoteData::Null => {}
        }

        env.storage().persistent().set(&prop_key, &proposal);

        // Update total votes
        let total_key = (Symbol::new(&env, "total_v"), proposal_id);
        let mut total: i128 = env.storage().persistent().get(&total_key).unwrap_or(0);
        total += voting_power;
        env.storage().persistent().set(&total_key, &total);

        env.events().publish(
            (Symbol::new(&env, "dao"), Symbol::new(&env, "voted")),
            (caller, proposal_id, voting_power, vote),
        );
    }

    /// Tally votes for a proposal.
    pub fn tally_votes(env: Env, caller: Address, proposal_id: u32) {
        caller.require_auth();

        let prop_key = (Symbol::new(&env, "prop"), proposal_id);
        let proposal: ProposalData = env
            .storage()
            .persistent()
            .get(&prop_key)
            .unwrap_or_else(|| panic!("{:?}", DaoError::ProposalNotFound));

        if env.ledger().timestamp() < proposal.ends_at {
            panic!("{:?}", DaoError::ProposalNotEnded);
        }

        let total_key = (Symbol::new(&env, "total_v"), proposal_id);
        let total: i128 = env.storage().persistent().get(&total_key).unwrap_or(0);

        let tally_key = (Symbol::new(&env, "tally"), proposal_id);
        env.storage().persistent().set(&tally_key, &true);

        env.events().publish(
            (Symbol::new(&env, "dao"), Symbol::new(&env, "votes_tallied")),
            (proposal_id, total),
        );
    }

    /// Execute a proposal by calling Investment.create_investment via cross-contract call.
    pub fn execute_proposal(
        env: Env,
        caller: Address,
        proposal_id: u32,
        farm_id: u32,
        name: String,
        about: String,
        min_amount: i128,
        end_date: u64,
    ) {
        caller.require_auth();

        let prop_key = (Symbol::new(&env, "prop"), proposal_id);
        let mut proposal: ProposalData = env
            .storage()
            .persistent()
            .get(&prop_key)
            .unwrap_or_else(|| panic!("{:?}", DaoError::ProposalNotFound));

        let tally_key = (Symbol::new(&env, "tally"), proposal_id);
        if !env
            .storage()
            .persistent()
            .get::<_, bool>(&tally_key)
            .unwrap_or(false)
        {
            panic!("{:?}", DaoError::VotesNotTallied);
        }

        if proposal.executed {
            panic!("{:?}", DaoError::ProposalAlreadyExecuted);
        }

        if proposal.accept_votes < proposal.required_votes {
            panic!("{:?}", DaoError::InsufficientVotes);
        }

        // Cross-contract call to Investment contract
        let investment_addr: Address = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "investment"))
            .unwrap();

        let image = String::from_str(&env, "");
        env.invoke_contract::<()>(
            &investment_addr,
            &Symbol::new(&env, "create_investment"),
            vec![
                &env,
                farm_id.into_val(&env),
                image.into_val(&env),
                name.into_val(&env),
                about.into_val(&env),
                min_amount.into_val(&env),
                end_date.into_val(&env),
                proposal.proposer.clone().into_val(&env),
            ],
        );

        proposal.executed = true;
        env.storage().persistent().set(&prop_key, &proposal);

        env.events().publish(
            (
                Symbol::new(&env, "dao"),
                Symbol::new(&env, "proposal_executed"),
            ),
            (proposal_id, farm_id, proposal.proposer),
        );
    }

    /// Delegate voting power.
    pub fn delegate(env: Env, caller: Address, delegatee: Address) {
        caller.require_auth();

        if caller == delegatee {
            panic!("{:?}", DaoError::CannotDelegateToSelf);
        }

        let deleg_key = (Symbol::new(&env, "deleg"), caller.clone());
        if env.storage().persistent().has(&deleg_key) {
            panic!("{:?}", DaoError::AlreadyDelegated);
        }

        env.storage().persistent().set(&deleg_key, &delegatee);

        env.events().publish(
            (Symbol::new(&env, "dao"), Symbol::new(&env, "delegated")),
            (caller, delegatee),
        );
    }

    /// Remove delegation.
    pub fn undelegate(env: Env, caller: Address) {
        caller.require_auth();

        let deleg_key = (Symbol::new(&env, "deleg"), caller.clone());
        if !env.storage().persistent().has(&deleg_key) {
            panic!("{:?}", DaoError::NotDelegated);
        }

        env.storage().persistent().remove(&deleg_key);

        env.events().publish(
            (Symbol::new(&env, "dao"), Symbol::new(&env, "undelegated")),
            caller,
        );
    }

    /// Get delegate for a delegator.
    pub fn get_delegate(env: Env, delegator: Address) -> Address {
        let deleg_key = (Symbol::new(&env, "deleg"), delegator);
        env.storage()
            .persistent()
            .get(&deleg_key)
            .unwrap_or_else(|| panic!("{:?}", DaoError::NotDelegated))
    }

    /// Create a challenge against a proposal.
    pub fn create_challenge(env: Env, caller: Address, proposal_id: u32, description: String) {
        caller.require_auth();

        if description.is_empty() {
            panic!("{:?}", DaoError::DescriptionCannotBeEmpty);
        }

        let mut chall_counter: u32 = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "chall_ctr"))
            .unwrap_or(0);
        chall_counter += 1;

        let challenge = ChallengeData {
            proposal_id,
            description,
            resolved: false,
            valid: false,
            challenger: caller.clone(),
        };

        let chall_key = (Symbol::new(&env, "chall"), chall_counter);
        env.storage().persistent().set(&chall_key, &challenge);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "chall_ctr"), &chall_counter);

        // Mark proposal as challenged
        let prop_key = (Symbol::new(&env, "prop"), proposal_id);
        let mut proposal: ProposalData = env
            .storage()
            .persistent()
            .get(&prop_key)
            .unwrap_or_else(|| panic!("{:?}", DaoError::ProposalNotFound));
        proposal.is_challenged = true;
        env.storage().persistent().set(&prop_key, &proposal);

        env.events().publish(
            (
                Symbol::new(&env, "dao"),
                Symbol::new(&env, "challenge_created"),
            ),
            (chall_counter, proposal_id, caller),
        );
    }

    /// Resolve a challenge. Only admin can resolve.
    pub fn resolve_challenge(env: Env, caller: Address, challenge_id: u32, valid: bool) {
        caller.require_auth();

        let admin: Address = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "admin"))
            .unwrap();
        if caller != admin {
            panic!("{:?}", DaoError::NotAdmin);
        }

        let chall_key = (Symbol::new(&env, "chall"), challenge_id);
        let mut challenge: ChallengeData = env
            .storage()
            .persistent()
            .get(&chall_key)
            .unwrap_or_else(|| panic!("{:?}", DaoError::ChallengeNotFound));

        if challenge.resolved {
            panic!("{:?}", DaoError::ChallengeAlreadyResolved);
        }

        challenge.resolved = true;
        challenge.valid = valid;
        env.storage().persistent().set(&chall_key, &challenge);

        env.events().publish(
            (
                Symbol::new(&env, "dao"),
                Symbol::new(&env, "challenge_resolved"),
            ),
            (challenge_id, valid),
        );
    }

    /// Get challenge details.
    pub fn get_challenge(env: Env, challenge_id: u32) -> ChallengeData {
        let chall_key = (Symbol::new(&env, "chall"), challenge_id);
        env.storage()
            .persistent()
            .get(&chall_key)
            .unwrap_or_else(|| panic!("{:?}", DaoError::ChallengeNotFound))
    }

    /// Initiate a dispute for a challenge.
    pub fn initiate_dispute(env: Env, caller: Address, challenge_id: u32, arbitrator: Address) {
        caller.require_auth();

        let mut disp_counter: u32 = env
            .storage()
            .instance()
            .get(&Symbol::new(&env, "disp_ctr"))
            .unwrap_or(0);
        disp_counter += 1;

        let dispute = DisputeData {
            challenge_id,
            arbitrator,
            resolved: false,
            ruling: false,
        };

        let disp_key = (Symbol::new(&env, "disp"), disp_counter);
        env.storage().persistent().set(&disp_key, &dispute);
        env.storage()
            .instance()
            .set(&Symbol::new(&env, "disp_ctr"), &disp_counter);

        env.events().publish(
            (
                Symbol::new(&env, "dao"),
                Symbol::new(&env, "dispute_initiated"),
            ),
            (disp_counter, challenge_id, caller),
        );
    }

    /// Resolve a dispute.
    pub fn resolve_dispute(env: Env, caller: Address, dispute_id: u32, ruling: bool) {
        caller.require_auth();

        let disp_key = (Symbol::new(&env, "disp"), dispute_id);
        let mut dispute: DisputeData = env
            .storage()
            .persistent()
            .get(&disp_key)
            .unwrap_or_else(|| panic!("{:?}", DaoError::DisputeNotFound));

        if dispute.resolved {
            panic!("{:?}", DaoError::DisputeAlreadyResolved);
        }

        if caller != dispute.arbitrator {
            panic!("{:?}", DaoError::InvalidArbitrator);
        }

        dispute.resolved = true;
        dispute.ruling = ruling;
        env.storage().persistent().set(&disp_key, &dispute);

        env.events().publish(
            (
                Symbol::new(&env, "dao"),
                Symbol::new(&env, "dispute_resolved"),
            ),
            (dispute_id, ruling),
        );
    }

    /// Get dispute details.
    pub fn get_dispute(env: Env, dispute_id: u32) -> DisputeData {
        let disp_key = (Symbol::new(&env, "disp"), dispute_id);
        env.storage()
            .persistent()
            .get(&disp_key)
            .unwrap_or_else(|| panic!("{:?}", DaoError::DisputeNotFound))
    }
}

#[cfg(test)]
mod test;
