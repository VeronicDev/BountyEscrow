#![cfg(test)]
extern crate std;

use super::*;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{token, Env, String as SorobanString};

fn create_token<'a>(env: &Env, admin: &Address) -> (token::Client<'a>, token::StellarAssetClient<'a>) {
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    (
        token::Client::new(env, &sac.address()),
        token::StellarAssetClient::new(env, &sac.address()),
    )
}

struct Setup<'a> {
    env: Env,
    admin: Address,
    payer: Address,
    agent: Address,
    contributor: Address,
    token: token::Client<'a>,
    contract: BountyEscrowContractClient<'a>,
}

fn setup() -> Setup<'static> {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let payer = Address::generate(&env);
    let agent = Address::generate(&env);
    let contributor = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let (token, token_admin_client) = create_token(&env, &token_admin);
    token_admin_client.mint(&payer, &1_000_000);

    let contract_id = env.register(BountyEscrowContract, (admin.clone(), token.address.clone()));
    let contract = BountyEscrowContractClient::new(&env, &contract_id);

    Setup {
        env,
        admin,
        payer,
        agent,
        contributor,
        token,
        contract,
    }
}

#[test]
fn test_create_bounty_escrows_funds() {
    let s = setup();
    let issue_ref = SorobanString::from_str(&s.env, "org/repo#42");

    let id = s.contract.create_bounty(&s.payer, &s.agent, &issue_ref, &500);

    assert_eq!(id, 0);
    assert_eq!(s.token.balance(&s.payer), 1_000_000 - 500);
    assert_eq!(s.token.balance(&s.contract.address), 500);

    let bounty = s.contract.get_bounty(&id);
    assert_eq!(bounty.status, BountyStatus::Funded);
    assert_eq!(bounty.amount, 500);
    assert_eq!(bounty.payer, s.payer);
    assert_eq!(bounty.agent, s.agent);
}

#[test]
fn test_full_happy_path_releases_to_contributor() {
    let s = setup();
    let issue_ref = SorobanString::from_str(&s.env, "org/repo#42");
    let pr_url = SorobanString::from_str(&s.env, "https://github.com/org/repo/pull/7");

    let id = s.contract.create_bounty(&s.payer, &s.agent, &issue_ref, &500);
    s.contract.submit_pr(&id, &s.contributor, &pr_url);

    let bounty = s.contract.get_bounty(&id);
    assert_eq!(bounty.status, BountyStatus::Submitted);
    assert_eq!(bounty.contributor, Some(s.contributor.clone()));

    s.contract.release(&id);

    let bounty = s.contract.get_bounty(&id);
    assert_eq!(bounty.status, BountyStatus::Released);
    assert_eq!(s.token.balance(&s.contributor), 500);
    assert_eq!(s.token.balance(&s.contract.address), 0);
}

#[test]
fn test_dispute_then_admin_refund() {
    let s = setup();
    let issue_ref = SorobanString::from_str(&s.env, "org/repo#42");
    let pr_url = SorobanString::from_str(&s.env, "https://github.com/org/repo/pull/7");

    let id = s.contract.create_bounty(&s.payer, &s.agent, &issue_ref, &500);
    s.contract.submit_pr(&id, &s.contributor, &pr_url);
    s.contract.dispute(&id);

    let bounty = s.contract.get_bounty(&id);
    assert_eq!(bounty.status, BountyStatus::Disputed);

    // Agent can no longer release once disputed.
    let release_result = s.contract.try_release(&id);
    assert_eq!(release_result, Err(Ok(Error::InvalidState)));

    s.contract.refund(&id);
    let bounty = s.contract.get_bounty(&id);
    assert_eq!(bounty.status, BountyStatus::Refunded);
    assert_eq!(s.token.balance(&s.payer), 1_000_000);
    assert_eq!(s.token.balance(&s.contract.address), 0);
}

#[test]
fn test_release_before_submission_fails() {
    let s = setup();
    let issue_ref = SorobanString::from_str(&s.env, "org/repo#42");
    let id = s.contract.create_bounty(&s.payer, &s.agent, &issue_ref, &500);

    let result = s.contract.try_release(&id);
    assert_eq!(result, Err(Ok(Error::InvalidState)));
}

#[test]
fn test_zero_amount_rejected() {
    let s = setup();
    let issue_ref = SorobanString::from_str(&s.env, "org/repo#42");
    let result = s.contract.try_create_bounty(&s.payer, &s.agent, &issue_ref, &0);
    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
}
