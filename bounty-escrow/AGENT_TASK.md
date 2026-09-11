# Task: Verify, fix, and deploy the BountyEscrow Soroban contract

## Context

This repo contains a Soroban (Stellar smart contract) project called
**BountyEscrow** — an AI-agent-verified bounty escrow for open-source
contributions. Flow: a payer funds a bounty tied to a GitHub issue, a
contributor submits a PR, and a designated "agent" address (an
off-chain service, built separately, not part of this task) evaluates
the PR and calls `release` to pay the contributor out of escrow. A
payer can `dispute` a submission before release; an admin can
`refund` a disputed or unclaimed bounty.

The contract already exists in `contracts/bounty-escrow/src/lib.rs`,
with tests in `contracts/bounty-escrow/src/test.rs`. It was written
directly against Stellar's current official smart-contracts dev docs
(soroban-sdk 27.0.6, protocol 27 patterns — storage/TTL, auth trees,
SEP-41 token transfers via `TokenClient`). **It has not been compiled
or run yet** — the environment it was written in only had Rust 1.75
available and no network access to install a newer toolchain, so this
is an unverified first draft, not a rewrite target.

## What I need you to do

1. Set up the toolchain: Rust 1.84+ via `rustup`, the `wasm32v1-none`
   target, and `stellar-cli` v27+ (`cargo install --locked stellar-cli`
   or the installer at developers.stellar.org).
2. Run `cargo test` inside `contracts/bounty-escrow` and fix any
   compile or test errors. Preserve the existing design — storage
   keys, auth model (`payer`/`agent`/`admin` roles), event shapes,
   function signatures, the `Funded → Submitted →
   Released/Disputed/Refunded` status lifecycle — unless something is
   actually broken. This is a verify-and-fix pass, not a redesign.
3. Once tests pass, run `stellar contract build` and confirm the
   output `.wasm` is under the network's 128KB contract size limit.
4. Deploy to Stellar **testnet**: generate a funded identity, deploy
   or reference a test SEP-41 token (a testnet SAC is fine), and
   smoke-test the full flow via `stellar contract invoke`:
   `create_bounty` → `submit_pr` → `release`, confirming token
   balances move correctly at each step. Also exercise `dispute` +
   `refund` once.
5. Report back: what you had to fix (if anything) and why, the
   deployed testnet contract ID, and the exact commands you ran, so
   the result is reproducible.

## Constraints

- Don't add features or change the bounty lifecycle — get this exact
  design working first.
- If you hit an actual design flaw (not just a syntax/version/API
  drift issue), flag it clearly rather than silently changing
  behavior — I want to know about it, not have it quietly patched
  over.
- Keep commits/diffs minimal and explain each fix inline (comment or
  commit message) so I can tell "this was a real bug" from "this was
  an SDK API rename since the doc I wrote it against."

## Reference

The contract was written against:
https://github.com/stellar/stellar-dev-skill/tree/main/skills/smart-contracts
(`SKILL.md`, `development.md`, `testing.md`) — if soroban-sdk has
moved since, that's the first place to check for what changed.
