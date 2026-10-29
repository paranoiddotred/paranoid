//! Turning the burn vault's USDC into burned PARANOID without a USDC pool of its own.
//! Two permissionless steps. `convert_burn` sells vault USDC for wSOL at Pyth's SOL/USD price,
//! and the caller keeps `BURN_KEEPER_BPS` of the price as its pay, so arbitrage keeps the vault
//! moving. `crank_burn` then swaps the gathered wSOL for PARANOID in the configured Meteora pool.
//! Half of the tokens bought go to stakers that are not unstaking, in proportion to their stake, and
//! the rest is burned; while nobody stakes, all of it is burned. The oracle prices the big leg; the pool
//! leg is kept small instead: `BURN_MAX_POOL_BPS` of the pool's SOL side per cooldown. Pushing the price
//! first costs a mover the pool's fee on both legs (2% each), about ten times what the crank overpays.

use anchor_lang::prelude::*;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};
use anchor_spl::token_interface::{self, Burn, Mint as AnyMint, TokenAccount as AnyTokenAccount, TokenInterface, TransferChecked};

use crate::constants::*;
use crate::error::ParanoidError;
use crate::events::{BurnFunded, Burned, StakersPaid};
use crate::meteora::{self, read_damm_pool, read_sol_usd, tokens_out_floor};
use crate::state::{BurnRoute, Config, Stake, StakeReward};

#[derive(Accounts)]
pub struct ConvertBurn<'info> {
    pub caller: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(mut, seeds = [BURN_VAULT_SEED], bump)]
    pub burn_vault: Account<'info, TokenAccount>,
    /// Created by `set_burn_target`; until then the conversion fails here (AccountNotInitialized).
    #[account(mut, seeds = [WSOL_VAULT_SEED], bump)]
    pub wsol_vault: Account<'info, TokenAccount>,
    #[account(mut, constraint = caller_usdc.mint == USDC_MINT @ ParanoidError::BurnAmountInvalid)]
    pub caller_usdc: Account<'info, TokenAccount>,
    #[account(mut, constraint = caller_wsol.mint == meteora::WSOL_MINT @ ParanoidError::BurnAmountInvalid)]
    pub caller_wsol: Account<'info, TokenAccount>,
    /// CHECK: parsed by `read_sol_usd`, which checks the owner, the feed and the verification level.
    pub price_update: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
}

/// Anyone: buys `usdc` of the burn vault's USDC with wSOL at the oracle price, keeping
/// `BURN_KEEPER_BPS` of it as pay. The wSOL waits in the program's vault for `crank_burn`.
pub fn convert_burn(ctx: Context<ConvertBurn>, usdc: u64) -> Result<()> {
    require!(
        usdc > 0 && usdc <= ctx.accounts.burn_vault.amount && usdc <= BURN_MAX_PER_CALL,
        ParanoidError::BurnAmountInvalid
    );
    let (sol_usd, expo) = read_sol_usd(&ctx.accounts.price_update, Clock::get()?.unix_timestamp, PRICE_MAX_AGE_SECS)?;
    // lamports = usdc (1e-6 USD) x 1e3 x 10^-expo / price, then the caller's cut comes off.
    let scale = 10u128
        .checked_pow(u32::try_from(-expo).map_err(|_| ParanoidError::MathOverflow)?)
        .ok_or(ParanoidError::MathOverflow)?;
    let fair = u128::from(usdc)
        .checked_mul(1_000)
        .and_then(|v| v.checked_mul(scale))
        .ok_or(ParanoidError::MathOverflow)?
        / u128::from(sol_usd);
    let pay = u64::try_from(fair * u128::from(BPS - BURN_KEEPER_BPS) / u128::from(BPS))
        .map_err(|_| ParanoidError::MathOverflow)?;
    require!(pay > 0, ParanoidError::BurnAmountInvalid);

    token::transfer(
        CpiContext::new(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.caller_wsol.to_account_info(),
                to: ctx.accounts.wsol_vault.to_account_info(),
                authority: ctx.accounts.caller.to_account_info(),
            },
        ),
        pay,
    )?;
    let seeds: &[&[u8]] = &[CONFIG_SEED, &[ctx.accounts.config.bump]];
    token::transfer(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            Transfer {
                from: ctx.accounts.burn_vault.to_account_info(),
                to: ctx.accounts.caller_usdc.to_account_info(),
                authority: ctx.accounts.config.to_account_info(),
            },
            &[seeds],
        ),
        usdc,
    )?;

    let config = &mut ctx.accounts.config;
    config.burn_spent = config.burn_spent.checked_add(usdc).ok_or(ParanoidError::MathOverflow)?;
    emit!(BurnFunded { usdc, lamports: pay });
    Ok(())
}

#[derive(Accounts)]
pub struct CrankBurn<'info> {
    pub caller: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(mut, seeds = [BURN_ROUTE_SEED], bump = burn_route.bump)]
    pub burn_route: Account<'info, BurnRoute>,
    #[account(mut, seeds = [WSOL_VAULT_SEED], bump)]
    pub wsol_vault: Account<'info, TokenAccount>,
    #[account(mut, seeds = [BOUGHT_SEED], bump)]
    pub bought: InterfaceAccount<'info, AnyTokenAccount>,
    #[account(mut, address = config.burn_mint @ ParanoidError::BurnPoolInvalid)]
    pub burn_mint: InterfaceAccount<'info, AnyMint>,
    #[account(address = meteora::WSOL_MINT)]
    pub wsol_mint: Account<'info, Mint>,
    /// CHECK: the configured Meteora pool; parsed in the handler.
    #[account(mut, address = config.burn_pool @ ParanoidError::BurnPoolInvalid)]
    pub pool: UncheckedAccount<'info>,
    /// CHECK: matched against the pool's own vault keys.
    #[account(mut)]
    pub pool_vault_a: UncheckedAccount<'info>,
    /// CHECK: matched against the pool's own vault keys.
    #[account(mut)]
    pub pool_vault_b: UncheckedAccount<'info>,
    /// CHECK: DAMM v2's fixed pool authority.
    #[account(address = meteora::POOL_AUTHORITY)]
    pub pool_authority: UncheckedAccount<'info>,
    /// CHECK: DAMM v2's event authority PDA.
    #[account(address = meteora::EVENT_AUTHORITY)]
    pub event_authority: UncheckedAccount<'info>,
    /// CHECK: pinned to the DAMM v2 program id.
    #[account(address = meteora::DAMM_V2)]
    pub damm_program: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
    pub burn_token_program: Interface<'info, TokenInterface>,
    #[account(seeds = [STAKE_SEED], bump = stake.bump)]
    pub stake: Account<'info, Stake>,
    #[account(mut, seeds = [STAKE_REWARD_SEED], bump = stake_reward.bump)]
    pub stake_reward: Account<'info, StakeReward>,
    #[account(mut, seeds = [STAKE_REWARD_VAULT_SEED], bump)]
    pub stake_reward_vault: InterfaceAccount<'info, AnyTokenAccount>,
}

/// The amount in an SPL token account that is only passed through as `UncheckedAccount`.
fn token_amount(acc: &AccountInfo) -> Result<u64> {
    let d = acc.try_borrow_data()?;
    require!(d.len() >= 72, ParanoidError::BurnPoolInvalid);
    Ok(u64::from_le_bytes(d[64..72].try_into().unwrap()))
}

/// Anyone, once per cooldown: swaps the gathered wSOL (capped at a sliver of the pool's SOL side)
/// for PARANOID in the configured pool. `STAKER_SHARE_BPS` of the tokens bought go to the stakers that
/// are not unstaking and the rest is burned; with no stake earning, every token bought is burned.
pub fn crank_burn(ctx: Context<CrankBurn>) -> Result<()> {
    let slot = Clock::get()?.slot;
    let last = ctx.accounts.burn_route.last_swap_slot;
    // 0 means the swap has never run; a fresh chain's slot can still be under the cooldown.
    require!(
        last == 0 || slot >= last.saturating_add(BURN_COOLDOWN_SLOTS),
        ParanoidError::BurnTooSoon
    );
    let pool = read_damm_pool(&ctx.accounts.pool)?;
    require_keys_eq!(ctx.accounts.pool_vault_a.key(), pool.vault_a, ParanoidError::BurnPoolInvalid);
    require_keys_eq!(ctx.accounts.pool_vault_b.key(), pool.vault_b, ParanoidError::BurnPoolInvalid);
    let paranoid_is_a = pool.mint_a == ctx.accounts.config.burn_mint;

    let sol_side = token_amount(if paranoid_is_a { &ctx.accounts.pool_vault_b } else { &ctx.accounts.pool_vault_a })?;
    let cap = u64::try_from(u128::from(sol_side) * u128::from(BURN_MAX_POOL_BPS) / u128::from(BPS))
        .map_err(|_| ParanoidError::MathOverflow)?;
    let amount = ctx.accounts.wsol_vault.amount.min(cap);
    require!(amount > 0, ParanoidError::BurnAmountInvalid);

    let expected = tokens_out_floor(amount, pool.sqrt_price, paranoid_is_a).ok_or(ParanoidError::MathOverflow)?;
    let minimum_out = u64::try_from(u128::from(expected) * u128::from(BURN_MIN_OUT_BPS) / u128::from(BPS))
        .map_err(|_| ParanoidError::MathOverflow)?;
    require!(minimum_out > 0, ParanoidError::BurnAmountInvalid);

    // The CPI's a and b follow the pool's own order.
    let (mint_a, mint_b, program_a, program_b) = if paranoid_is_a {
        (
            ctx.accounts.burn_mint.to_account_info(),
            ctx.accounts.wsol_mint.to_account_info(),
            ctx.accounts.burn_token_program.to_account_info(),
            ctx.accounts.token_program.to_account_info(),
        )
    } else {
        (
            ctx.accounts.wsol_mint.to_account_info(),
            ctx.accounts.burn_mint.to_account_info(),
            ctx.accounts.token_program.to_account_info(),
            ctx.accounts.burn_token_program.to_account_info(),
        )
    };
    let seeds: &[&[u8]] = &[CONFIG_SEED, &[ctx.accounts.config.bump]];
    meteora::swap(
        &ctx.accounts.damm_program.to_account_info(),
        &ctx.accounts.pool_authority.to_account_info(),
        &ctx.accounts.pool.to_account_info(),
        &ctx.accounts.wsol_vault.to_account_info(),
        &ctx.accounts.bought.to_account_info(),
        &ctx.accounts.pool_vault_a.to_account_info(),
        &ctx.accounts.pool_vault_b.to_account_info(),
        &mint_a,
        &mint_b,
        &ctx.accounts.config.to_account_info(),
        &program_a,
        &program_b,
        &ctx.accounts.event_authority.to_account_info(),
        amount,
        minimum_out,
        &[seeds],
    )?;

    // Hands out and burns everything in `bought`, including anything sent to it from outside.
    ctx.accounts.bought.reload()?;
    let bought = ctx.accounts.bought.amount;
    ctx.accounts.burn_route.last_swap_slot = slot;
    let earning = ctx.accounts.stake_reward.earning;
    let to_stakers = if earning > 0 && ctx.accounts.burn_mint.key() == PARANOID_MINT {
        u64::try_from(u128::from(bought) * u128::from(STAKER_SHARE_BPS) / u128::from(BPS)).map_err(|_| ParanoidError::MathOverflow)?
    } else {
        0
    };
    if to_stakers > 0 {
        token_interface::transfer_checked(
            CpiContext::new_with_signer(
                ctx.accounts.burn_token_program.to_account_info(),
                TransferChecked {
                    from: ctx.accounts.bought.to_account_info(),
                    mint: ctx.accounts.burn_mint.to_account_info(),
                    to: ctx.accounts.stake_reward_vault.to_account_info(),
                    authority: ctx.accounts.config.to_account_info(),
                },
                &[seeds],
            ),
            to_stakers,
            ctx.accounts.burn_mint.decimals,
        )?;
        let reward = &mut ctx.accounts.stake_reward;
        let step = u128::from(to_stakers).checked_mul(REWARD_SCALE).ok_or(ParanoidError::MathOverflow)? / u128::from(earning);
        reward.per_token = reward.per_token.checked_add(step).ok_or(ParanoidError::MathOverflow)?;
        reward.given = reward.given.checked_add(to_stakers).ok_or(ParanoidError::MathOverflow)?;
        emit!(StakersPaid { lamports_spent: amount, tokens: to_stakers, earning });
    }
    let burned = bought - to_stakers;
    token_interface::burn(
        CpiContext::new_with_signer(
            ctx.accounts.burn_token_program.to_account_info(),
            Burn {
                mint: ctx.accounts.burn_mint.to_account_info(),
                from: ctx.accounts.bought.to_account_info(),
                authority: ctx.accounts.config.to_account_info(),
            },
            &[seeds],
        ),
        burned,
    )?;

    let config = &mut ctx.accounts.config;
    config.token_burned = config.token_burned.checked_add(burned).ok_or(ParanoidError::MathOverflow)?;
    emit!(Burned { lamports_spent: amount, token_burned: burned });
    Ok(())
}
