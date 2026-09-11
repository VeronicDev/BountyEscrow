#![no_std]

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, token::TokenClient,
    Address, Env, String,
};

mod test;

// ---------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Token,
    NextId,
    Bounty(u64),
}

// TTL tuning. Bounties are meant to live for a Wave-scale timeframe (days
// to a few weeks), not the max the network allows — but we extend
// generously so nothing gets archived mid-flow. Ledgers are ~5s apart,
// so 17,280 ledgers ~= 1 day.
const DAY_LEDGERS: u32 = 17_280;
const BUMP_THRESHOLD: u32 = 30 * DAY_LEDGERS;
const BUMP_TO: u32 = 90 * DAY_LEDGERS;
const INSTANCE_BUMP_THRESHOLD: u32 = 30 * DAY_LEDGERS;
const INSTANCE_BUMP_TO: u32 = 90 * DAY_LEDGERS;

// ---------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BountyStatus {
    Funded,
    Submitted,
    Released,
    Disputed,
    Refunded,
}

#[contracttype]
#[derive(Clone)]
pub struct Bounty {
    pub payer: Address,
    pub agent: Address,
    pub contributor: Option<Address>,
    pub issue_ref: String,
    pub pr_url: Option<String>,
    pub amount: i128,
    pub status: BountyStatus,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    InvalidAmount = 2,
    InvalidState = 3,
    NotFound = 4,
    NoContributor = 5,
}

// ---------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------

#[contractevent]
pub struct BountyCreated {
    #[topic]
    pub bounty_id: u64,
    #[topic]
    pub payer: Address,
    pub agent: Address,
    pub amount: i128,
}

#[contractevent]
pub struct PrSubmitted {
    #[topic]
    pub bounty_id: u64,
    #[topic]
    pub contributor: Address,
    pub pr_url: String,
}

#[contractevent]
pub struct BountyReleased {
    #[topic]
    pub bounty_id: u64,
    #[topic]
    pub contributor: Address,
    pub amount: i128,
}

#[contractevent]
pub struct BountyDisputed {
    #[topic]
    pub bounty_id: u64,
}

#[contractevent]
pub struct BountyRefunded {
    #[topic]
    pub bounty_id: u64,
}

// ---------------------------------------------------------------------
// Contract
// ---------------------------------------------------------------------

#[contract]
pub struct BountyEscrowContract;

#[contractimpl]
impl BountyEscrowContract {
    /// Runs once at deploy time. `admin` is the arbitration authority
    /// (can refund a disputed or unclaimed bounty); `token` is the SEP-41
    /// token address bounties are denominated in (e.g. a USDC SAC, or
    /// native XLM's SAC on this network).
    pub fn __constructor(env: Env, admin: Address, token: Address) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::NextId, &0u64);
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_BUMP_THRESHOLD, INSTANCE_BUMP_TO);
    }

    /// Payer funds a new bounty for `issue_ref`, naming `agent` as the
    /// only address allowed to release it once work is submitted. Funds
    /// move from payer to the contract atomically with creation.
    pub fn create_bounty(
        env: Env,
        payer: Address,
        agent: Address,
        issue_ref: String,
        amount: i128,
    ) -> Result<u64, Error> {
        payer.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let token: Address = env
            .storage()
            .instance()
            .get(&DataKey::Token)
            .ok_or(Error::NotInitialized)?;
        let token_client = TokenClient::new(&env, &token);
        token_client.transfer(&payer, &env.current_contract_address(), &amount);

        let id: u64 = env.storage().instance().get(&DataKey::NextId).unwrap_or(0);
        env.storage().instance().set(&DataKey::NextId, &(id + 1));
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_BUMP_THRESHOLD, INSTANCE_BUMP_TO);

        let bounty = Bounty {
            payer: payer.clone(),
            agent: agent.clone(),
            contributor: None,
            issue_ref,
            pr_url: None,
            amount,
            status: BountyStatus::Funded,
        };
        let key = DataKey::Bounty(id);
        env.storage().persistent().set(&key, &bounty);
        env.storage()
            .persistent()
            .extend_ttl(&key, BUMP_THRESHOLD, BUMP_TO);

        BountyCreated {
            bounty_id: id,
            payer,
            agent,
            amount,
        }
        .publish(&env);

        Ok(id)
    }

    /// Contributor submits their PR against a funded bounty. This does
    /// not move funds — it just puts the bounty in front of the agent
    /// for evaluation.
    pub fn submit_pr(
        env: Env,
        bounty_id: u64,
        contributor: Address,
        pr_url: String,
    ) -> Result<(), Error> {
        contributor.require_auth();

        let key = DataKey::Bounty(bounty_id);
        let mut bounty: Bounty = env.storage().persistent().get(&key).ok_or(Error::NotFound)?;

        if bounty.status != BountyStatus::Funded {
            return Err(Error::InvalidState);
        }

        bounty.contributor = Some(contributor.clone());
        bounty.pr_url = Some(pr_url.clone());
        bounty.status = BountyStatus::Submitted;
        env.storage().persistent().set(&key, &bounty);
        env.storage()
            .persistent()
            .extend_ttl(&key, BUMP_THRESHOLD, BUMP_TO);

        PrSubmitted {
            bounty_id,
            contributor,
            pr_url,
        }
        .publish(&env);

        Ok(())
    }

    /// Releases a submitted bounty to the contributor. Only the agent
    /// named at creation time can call this — this is the one function
    /// the off-chain evaluator invokes after it decides the PR satisfies
    /// the issue.
    pub fn release(env: Env, bounty_id: u64) -> Result<(), Error> {
        let key = DataKey::Bounty(bounty_id);
        let mut bounty: Bounty = env.storage().persistent().get(&key).ok_or(Error::NotFound)?;

        bounty.agent.require_auth();

        if bounty.status != BountyStatus::Submitted {
            return Err(Error::InvalidState);
        }
        let contributor = bounty.contributor.clone().ok_or(Error::NoContributor)?;

        let token: Address = env
            .storage()
            .instance()
            .get(&DataKey::Token)
            .ok_or(Error::NotInitialized)?;
        let token_client = TokenClient::new(&env, &token);
        token_client.transfer(&env.current_contract_address(), &contributor, &bounty.amount);

        bounty.status = BountyStatus::Released;
        let amount = bounty.amount;
        env.storage().persistent().set(&key, &bounty);
        env.storage()
            .persistent()
            .extend_ttl(&key, BUMP_THRESHOLD, BUMP_TO);

        BountyReleased {
            bounty_id,
            contributor,
            amount,
        }
        .publish(&env);

        Ok(())
    }

    /// Payer flags a submitted bounty as disputed, blocking the agent
    /// from releasing it until an admin resolves the dispute (via
    /// `refund`, or a future arbitration path).
    pub fn dispute(env: Env, bounty_id: u64) -> Result<(), Error> {
        let key = DataKey::Bounty(bounty_id);
        let mut bounty: Bounty = env.storage().persistent().get(&key).ok_or(Error::NotFound)?;

        bounty.payer.require_auth();

        if bounty.status != BountyStatus::Submitted {
            return Err(Error::InvalidState);
        }

        bounty.status = BountyStatus::Disputed;
        env.storage().persistent().set(&key, &bounty);
        env.storage()
            .persistent()
            .extend_ttl(&key, BUMP_THRESHOLD, BUMP_TO);

        BountyDisputed { bounty_id }.publish(&env);

        Ok(())
    }

    /// Admin-only escape hatch: returns funds to the payer for a bounty
    /// that's still unclaimed (`Funded`) or was disputed. This is the
    /// arbitration backstop — a full arbitration flow (e.g. letting an
    /// admin release a disputed bounty to the contributor instead) is a
    /// natural follow-up but deliberately left out of this first cut.
    pub fn refund(env: Env, bounty_id: u64) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        let key = DataKey::Bounty(bounty_id);
        let mut bounty: Bounty = env.storage().persistent().get(&key).ok_or(Error::NotFound)?;

        if bounty.status != BountyStatus::Funded && bounty.status != BountyStatus::Disputed {
            return Err(Error::InvalidState);
        }

        let token: Address = env
            .storage()
            .instance()
            .get(&DataKey::Token)
            .ok_or(Error::NotInitialized)?;
        let token_client = TokenClient::new(&env, &token);
        token_client.transfer(&env.current_contract_address(), &bounty.payer, &bounty.amount);

        bounty.status = BountyStatus::Refunded;
        env.storage().persistent().set(&key, &bounty);
        env.storage()
            .persistent()
            .extend_ttl(&key, BUMP_THRESHOLD, BUMP_TO);

        BountyRefunded { bounty_id }.publish(&env);

        Ok(())
    }

    /// Read-only lookup for the frontend / agent service.
    pub fn get_bounty(env: Env, bounty_id: u64) -> Result<Bounty, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Bounty(bounty_id))
            .ok_or(Error::NotFound)
    }
}
