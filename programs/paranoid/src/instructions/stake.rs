//! Stake: hold $PARANOID in the program and share the buybacks. Every purchase of questions buys
//! $PARANOID (burn.rs); half of each buy goes to the stakes that are not unstaking, in proportion to
//! their size, and the owner claims it any time. A stake belongs to a wallet and touches no membership.
//! Asking to unstake stops the earning; the tokens can be taken back 3 days (epochs) later.
//! Stakes from before 2026-10-27 sat on a membership and earned questions; `return_legacy_stake` gives
//! their tokens back at once.

use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

use crate::constants::*;
use crate::error::ParanoidError;
use crate::events::{LegacyStakeReturned, StakeAdded, StakeRewardClaimed, StakeWithdrawn, UnstakeRequested};
use crate::state::{Config, Member, Stake, StakePosition, StakeReward, WalletStake};

#[derive(Accounts)]
pub struct OpenStake<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(init, payer = payer, space = 8 + Stake::INIT_SPACE, seeds = [STAKE_SEED], bump)]
    pub stake: Account<'info, Stake>,
    #[account(address = PARANOID_MINT)]
    pub mint: Account<'info, Mint>,
    #[account(init, payer = payer, seeds = [STAKE_VAULT_SEED], bump, token::mint = mint, token::authority = stake)]
    pub stake_vault: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

/// Anyone, once: creates the stake and the account that holds staked tokens. Nothing is configurable.
pub fn open_stake(ctx: Context<OpenStake>) -> Result<()> {
    ctx.accounts.stake.set_inner(Stake { bump: ctx.bumps.stake, staked: 0, positions: 0, questions_given: 0 });
    Ok(())
}

#[derive(Accounts)]
pub struct OpenStakeReward<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    #[account(seeds = [STAKE_SEED], bump = stake.bump)]
    pub stake: Account<'info, Stake>,
    #[account(init, payer = payer, space = 8 + StakeReward::INIT_SPACE, seeds = [STAKE_REWARD_SEED], bump)]
    pub stake_reward: Account<'info, StakeReward>,
    #[account(address = PARANOID_MINT)]
    pub mint: Account<'info, Mint>,
    #[account(init, payer = payer, seeds = [STAKE_REWARD_VAULT_SEED], bump, token::mint = mint, token::authority = stake)]
    pub stake_reward_vault: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

/// Anyone, once: creates the share of bought tokens and the account that holds it. Wallet stakes need
/// it, so every one of them is counted in `earning` from its start.
pub fn open_stake_reward(ctx: Context<OpenStakeReward>) -> Result<()> {
    ctx.accounts.stake_reward.set_inner(StakeReward { bump: ctx.bumps.stake_reward, per_token: 0, earning: 0, given: 0, claimed: 0 });
    Ok(())
}

#[derive(Accounts)]
pub struct StartStake<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(init, payer = owner, space = 8 + WalletStake::INIT_SPACE, seeds = [WALLET_STAKE_SEED, owner.key().as_ref()], bump)]
    pub position: Account<'info, WalletStake>,
    #[account(mut, token::mint = PARANOID_MINT, token::authority = owner)]
    pub source: Account<'info, TokenAccount>,
    #[account(mut, seeds = [STAKE_SEED], bump = stake.bump)]
    pub stake: Account<'info, Stake>,
    #[account(mut, seeds = [STAKE_VAULT_SEED], bump)]
    pub stake_vault: Account<'info, TokenAccount>,
    #[account(mut, seeds = [STAKE_REWARD_SEED], bump = stake_reward.bump)]
    pub stake_reward: Account<'info, StakeReward>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

/// Owner: stakes `amount` tokens from the wallet. One stake per wallet; `add_stake` adds to it.
pub fn start_stake(ctx: Context<StartStake>, amount: u64) -> Result<()> {
    require!(amount > 0, ParanoidError::StakeTooSmall);
    let a = ctx.accounts;
    pull(&a.owner, &a.source, &a.stake_vault, &a.token_program, amount)?;
    a.position.set_inner(WalletStake {
        bump: ctx.bumps.position, owner: a.owner.key(), amount, unstake_at: 0, per_token_seen: a.stake_reward.per_token, tokens_owed: 0,
    });
    a.stake_reward.earning = a.stake_reward.earning.checked_add(amount).ok_or(ParanoidError::MathOverflow)?;
    a.stake.staked = a.stake.staked.checked_add(amount).ok_or(ParanoidError::MathOverflow)?;
    a.stake.positions = a.stake.positions.checked_add(1).ok_or(ParanoidError::MathOverflow)?;
    emit!(StakeAdded { owner: a.owner.key(), amount, staked: amount });
    Ok(())
}

#[derive(Accounts)]
pub struct AddStake<'info> {
    pub owner: Signer<'info>,
    #[account(mut, seeds = [WALLET_STAKE_SEED, owner.key().as_ref()], bump = position.bump, has_one = owner)]
    pub position: Account<'info, WalletStake>,
    #[account(mut, token::mint = PARANOID_MINT, token::authority = owner)]
    pub source: Account<'info, TokenAccount>,
    #[account(mut, seeds = [STAKE_SEED], bump = stake.bump)]
    pub stake: Account<'info, Stake>,
    #[account(mut, seeds = [STAKE_VAULT_SEED], bump)]
    pub stake_vault: Account<'info, TokenAccount>,
    #[account(mut, seeds = [STAKE_REWARD_SEED], bump = stake_reward.bump)]
    pub stake_reward: Account<'info, StakeReward>,
    pub token_program: Program<'info, Token>,
}

/// Owner: stakes `amount` more tokens. What the old amount earned so far is kept.
pub fn add_stake(ctx: Context<AddStake>, amount: u64) -> Result<()> {
    require!(amount > 0, ParanoidError::StakeTooSmall);
    require!(ctx.accounts.position.unstake_at == 0, ParanoidError::Unstaking);
    let a = ctx.accounts;
    settle_reward(&mut a.position, &a.stake_reward)?;
    pull(&a.owner, &a.source, &a.stake_vault, &a.token_program, amount)?;
    a.position.amount = a.position.amount.checked_add(amount).ok_or(ParanoidError::MathOverflow)?;
    a.stake_reward.earning = a.stake_reward.earning.checked_add(amount).ok_or(ParanoidError::MathOverflow)?;
    a.stake.staked = a.stake.staked.checked_add(amount).ok_or(ParanoidError::MathOverflow)?;
    emit!(StakeAdded { owner: a.owner.key(), amount, staked: a.position.amount });
    Ok(())
}

#[derive(Accounts)]
pub struct RequestUnstake<'info> {
    pub owner: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(mut, seeds = [WALLET_STAKE_SEED, owner.key().as_ref()], bump = position.bump, has_one = owner)]
    pub position: Account<'info, WalletStake>,
    #[account(mut, seeds = [STAKE_REWARD_SEED], bump = stake_reward.bump)]
    pub stake_reward: Account<'info, StakeReward>,
}

/// Owner: stops the stake earning and starts the 3-day (epoch) wait before the tokens can be taken
/// back. What it earned until now can still be claimed.
pub fn request_unstake(ctx: Context<RequestUnstake>) -> Result<()> {
    let a = ctx.accounts;
    require!(a.position.unstake_at == 0, ParanoidError::Unstaking);
    settle_reward(&mut a.position, &a.stake_reward)?;
    a.stake_reward.earning = a.stake_reward.earning.checked_sub(a.position.amount).ok_or(ParanoidError::MathOverflow)?;
    let wait = UNSTAKE_EPOCHS.checked_mul(i64::from(a.config.epoch_seconds)).ok_or(ParanoidError::MathOverflow)?;
    a.position.unstake_at = Clock::get()?.unix_timestamp.checked_add(wait).ok_or(ParanoidError::MathOverflow)?;
    emit!(UnstakeRequested { owner: a.owner.key(), amount: a.position.amount, unstake_at: a.position.unstake_at });
    Ok(())
}

#[derive(Accounts)]
pub struct WithdrawStake<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(mut, close = owner, seeds = [WALLET_STAKE_SEED, owner.key().as_ref()], bump = position.bump, has_one = owner)]
    pub position: Account<'info, WalletStake>,
    #[account(mut, token::mint = PARANOID_MINT, token::authority = owner)]
    pub destination: Account<'info, TokenAccount>,
    #[account(mut, seeds = [STAKE_SEED], bump = stake.bump)]
    pub stake: Account<'info, Stake>,
    #[account(mut, seeds = [STAKE_VAULT_SEED], bump)]
    pub stake_vault: Account<'info, TokenAccount>,
    #[account(mut, seeds = [STAKE_REWARD_SEED], bump = stake_reward.bump)]
    pub stake_reward: Account<'info, StakeReward>,
    #[account(mut, seeds = [STAKE_REWARD_VAULT_SEED], bump)]
    pub stake_reward_vault: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

/// Owner, 3 days after asking to unstake: takes every staked token back, plus the bought tokens it
/// earned and did not claim, and closes the stake.
pub fn withdraw_stake(ctx: Context<WithdrawStake>) -> Result<()> {
    let a = ctx.accounts;
    let unstake_at = a.position.unstake_at;
    require!(unstake_at != 0 && Clock::get()?.unix_timestamp >= unstake_at, ParanoidError::UnstakeNotReady);
    let amount = a.position.amount;
    pay_out(&a.stake_vault, &a.destination, &a.stake, &a.token_program, amount)?;
    claim_owed(&a.stake_reward_vault, &a.destination, &a.stake, &a.token_program, &mut a.position, &mut a.stake_reward)?;
    a.stake.staked = a.stake.staked.checked_sub(amount).ok_or(ParanoidError::MathOverflow)?;
    a.stake.positions = a.stake.positions.checked_sub(1).ok_or(ParanoidError::MathOverflow)?;
    emit!(StakeWithdrawn { owner: a.owner.key(), amount });
    Ok(())
}

#[derive(Accounts)]
pub struct ClaimStakeReward<'info> {
    pub owner: Signer<'info>,
    #[account(mut, seeds = [WALLET_STAKE_SEED, owner.key().as_ref()], bump = position.bump, has_one = owner)]
    pub position: Account<'info, WalletStake>,
    #[account(mut, token::mint = PARANOID_MINT, token::authority = owner)]
    pub destination: Account<'info, TokenAccount>,
    #[account(seeds = [STAKE_SEED], bump = stake.bump)]
    pub stake: Account<'info, Stake>,
    #[account(mut, seeds = [STAKE_REWARD_SEED], bump = stake_reward.bump)]
    pub stake_reward: Account<'info, StakeReward>,
    #[account(mut, seeds = [STAKE_REWARD_VAULT_SEED], bump)]
    pub stake_reward_vault: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

/// Owner: sends the bought $PARANOID the stake has earned to the owner's wallet. The stake stays as it
/// is. Works while unstaking too.
pub fn claim_stake_reward(ctx: Context<ClaimStakeReward>) -> Result<()> {
    let a = ctx.accounts;
    let paid = claim_owed(&a.stake_reward_vault, &a.destination, &a.stake, &a.token_program, &mut a.position, &mut a.stake_reward)?;
    require!(paid > 0, ParanoidError::NothingToClaim);
    Ok(())
}

#[derive(Accounts)]
pub struct ReturnLegacyStake<'info> {
    #[account(mut)]
    pub depositor: Signer<'info>,
    #[account(seeds = [MEMBER_SEED, member.commitment.as_ref()], bump = member.bump, has_one = depositor @ ParanoidError::NotDepositor)]
    pub member: Account<'info, Member>,
    #[account(mut, close = depositor, seeds = [STAKE_POSITION_SEED, member.key().as_ref()], bump = position.bump, has_one = member)]
    pub position: Account<'info, StakePosition>,
    #[account(mut, token::mint = PARANOID_MINT, token::authority = depositor)]
    pub destination: Account<'info, TokenAccount>,
    #[account(mut, seeds = [STAKE_SEED], bump = stake.bump)]
    pub stake: Account<'info, Stake>,
    #[account(mut, seeds = [STAKE_VAULT_SEED], bump)]
    pub stake_vault: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

/// Depositor of a membership with a stake from before 2026-10-27: takes every staked token back at
/// once and closes the old stake. Questions it earned and did not collect end with it.
pub fn return_legacy_stake(ctx: Context<ReturnLegacyStake>) -> Result<()> {
    let a = ctx.accounts;
    let amount = a.position.amount;
    pay_out(&a.stake_vault, &a.destination, &a.stake, &a.token_program, amount)?;
    a.stake.staked = a.stake.staked.checked_sub(amount).ok_or(ParanoidError::MathOverflow)?;
    a.stake.positions = a.stake.positions.checked_sub(1).ok_or(ParanoidError::MathOverflow)?;
    emit!(LegacyStakeReturned { commitment: a.member.commitment, amount });
    Ok(())
}

/// Adds the bought tokens the stake earned since it was last settled; a stake being unstaked earns nothing more.
fn settle_reward(position: &mut Account<WalletStake>, reward: &Account<StakeReward>) -> Result<()> {
    if position.unstake_at == 0 {
        let gain = u128::from(position.amount)
            .checked_mul(reward.per_token.checked_sub(position.per_token_seen).ok_or(ParanoidError::MathOverflow)?)
            .ok_or(ParanoidError::MathOverflow)?
            / REWARD_SCALE;
        position.tokens_owed = position.tokens_owed.checked_add(u64::try_from(gain).map_err(|_| ParanoidError::MathOverflow)?).ok_or(ParanoidError::MathOverflow)?;
    }
    position.per_token_seen = reward.per_token;
    Ok(())
}

/// Settles and sends every owed bought token to `destination`. Returns how many were sent.
fn claim_owed<'info>(
    vault: &Account<'info, TokenAccount>,
    destination: &Account<'info, TokenAccount>,
    stake: &Account<'info, Stake>,
    token_program: &Program<'info, Token>,
    position: &mut Account<'info, WalletStake>,
    reward: &mut Account<'info, StakeReward>,
) -> Result<u64> {
    settle_reward(position, reward)?;
    let tokens = position.tokens_owed;
    if tokens == 0 {
        return Ok(0);
    }
    pay_out(vault, destination, stake, token_program, tokens)?;
    position.tokens_owed = 0;
    reward.claimed = reward.claimed.checked_add(tokens).ok_or(ParanoidError::MathOverflow)?;
    emit!(StakeRewardClaimed { owner: position.owner, tokens });
    Ok(tokens)
}

/// Sends `amount` from a token account the stake PDA owns.
fn pay_out<'info>(
    from: &Account<'info, TokenAccount>,
    to: &Account<'info, TokenAccount>,
    stake: &Account<'info, Stake>,
    token_program: &Program<'info, Token>,
    amount: u64,
) -> Result<()> {
    let seeds: &[&[u8]] = &[STAKE_SEED, &[stake.bump]];
    token::transfer(
        CpiContext::new_with_signer(
            token_program.to_account_info(),
            Transfer { from: from.to_account_info(), to: to.to_account_info(), authority: stake.to_account_info() },
            &[seeds],
        ),
        amount,
    )
}

fn pull<'info>(owner: &Signer<'info>, source: &Account<'info, TokenAccount>, vault: &Account<'info, TokenAccount>, token_program: &Program<'info, Token>, amount: u64) -> Result<()> {
    token::transfer(
        CpiContext::new(
            token_program.to_account_info(),
            Transfer { from: source.to_account_info(), to: vault.to_account_info(), authority: owner.to_account_info() },
        ),
        amount,
    )
}
