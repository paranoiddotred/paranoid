//! PARANOID: buy AI questions with USDC, then ask with zero-knowledge proofs that some member is
//! spending one of its own unused tickets. The program sells tickets and keeps the member tree; the
//! proofs are checked off-chain by any gateway against the roots stored here. A member that spends
//! one ticket twice reveals its secret, and anyone can then slash it, which voids its other tickets.

// Anchor 0.31's `#[program]` expansion calls the deprecated `AccountInfo::realloc`; no code here does.
// `x % d != 0` stays over `is_multiple_of` so the build is byte-identical to the deployed program.
#![allow(deprecated, clippy::manual_is_multiple_of)]

use anchor_lang::prelude::*;

pub mod constants;
pub mod error;
pub mod events;
pub mod instructions;
pub mod math;
pub mod merkle;
pub mod meteora;
pub mod state;

use instructions::*;

declare_id!("9JJqzEoDGkTE8CMLATRw7CqFhcyWVegfhFuEeGfBfVGm");

#[program]
pub mod paranoid {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, params: LaunchParams) -> Result<()> {
        instructions::initialize::initialize(ctx, params)
    }

    pub fn set_burn_target(ctx: Context<SetBurnTarget>) -> Result<()> {
        instructions::initialize::set_burn_target(ctx)
    }

    pub fn reconfigure(ctx: Context<Reconfigure>, params: LaunchParams) -> Result<()> {
        instructions::initialize::reconfigure(ctx, params)
    }

    pub fn deposit(ctx: Context<Deposit>, commitment: [u8; 32], questions: u32) -> Result<()> {
        instructions::deposit::deposit(ctx, commitment, questions)
    }

    pub fn top_up(ctx: Context<TopUp>, questions: u32, siblings: Vec<[u8; 32]>) -> Result<()> {
        instructions::top_up::top_up(ctx, questions, siblings)
    }

    pub fn slash(ctx: Context<Slash>, secret: [u8; 32], siblings: Vec<[u8; 32]>) -> Result<()> {
        instructions::slash::slash(ctx, secret, siblings)
    }

    pub fn sweep_treasury(ctx: Context<SweepTreasury>) -> Result<()> {
        instructions::sweep::sweep_treasury(ctx)
    }

    pub fn convert_burn(ctx: Context<ConvertBurn>, usdc: u64) -> Result<()> {
        instructions::burn::convert_burn(ctx, usdc)
    }

    pub fn crank_burn(ctx: Context<CrankBurn>) -> Result<()> {
        instructions::burn::crank_burn(ctx)
    }

    pub fn set_token_payment(ctx: Context<SetTokenPayment>, premium_bps: i16) -> Result<()> {
        instructions::token_pay::set_token_payment(ctx, premium_bps)
    }

    pub fn set_token_premium(ctx: Context<SetTokenPremium>, premium_bps: i16) -> Result<()> {
        instructions::token_pay::set_token_premium(ctx, premium_bps)
    }

    pub fn deposit_token(ctx: Context<DepositToken>, commitment: [u8; 32], questions: u32, max_tokens: u64) -> Result<()> {
        instructions::token_pay::deposit_token(ctx, commitment, questions, max_tokens)
    }

    pub fn top_up_token(ctx: Context<TopUpToken>, questions: u32, siblings: Vec<[u8; 32]>, max_tokens: u64) -> Result<()> {
        instructions::token_pay::top_up_token(ctx, questions, siblings, max_tokens)
    }

    pub fn open_order(ctx: Context<OpenOrder>, amount: u64) -> Result<()> {
        instructions::order::open_order(ctx, amount)
    }

    pub fn settle_order(ctx: Context<SettleOrder>) -> Result<()> {
        instructions::order::settle_order(ctx)
    }

    pub fn open_stake(ctx: Context<OpenStake>) -> Result<()> {
        instructions::stake::open_stake(ctx)
    }

    pub fn open_stake_reward(ctx: Context<OpenStakeReward>) -> Result<()> {
        instructions::stake::open_stake_reward(ctx)
    }

    pub fn claim_stake_reward(ctx: Context<ClaimStakeReward>) -> Result<()> {
        instructions::stake::claim_stake_reward(ctx)
    }

    pub fn start_stake(ctx: Context<StartStake>, amount: u64) -> Result<()> {
        instructions::stake::start_stake(ctx, amount)
    }

    pub fn add_stake(ctx: Context<AddStake>, amount: u64) -> Result<()> {
        instructions::stake::add_stake(ctx, amount)
    }

    pub fn return_legacy_stake(ctx: Context<ReturnLegacyStake>) -> Result<()> {
        instructions::stake::return_legacy_stake(ctx)
    }

    pub fn request_unstake(ctx: Context<RequestUnstake>) -> Result<()> {
        instructions::stake::request_unstake(ctx)
    }

    pub fn withdraw_stake(ctx: Context<WithdrawStake>) -> Result<()> {
        instructions::stake::withdraw_stake(ctx)
    }
}
