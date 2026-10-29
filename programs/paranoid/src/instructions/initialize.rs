use anchor_lang::prelude::*;
use anchor_spl::token::{Mint, Token, TokenAccount};
use anchor_spl::token_interface::{Mint as AnyMint, TokenAccount as AnyTokenAccount, TokenInterface};

use crate::constants::*;
use crate::error::ParanoidError;
use crate::program::Paranoid;
use crate::meteora::{read_damm_pool, read_sol_usd, WSOL_MINT};
use crate::events::Reconfigured;
use crate::state::{BurnRoute, Config, Tree};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy)]
pub struct LaunchParams {
    /// Length of a day (epoch) in seconds.
    pub epoch_seconds: u32,
    /// Price of one question.
    pub rate: u64,
    /// Most tickets one membership may hold.
    pub limit: u16,
    /// Days a purchase keeps the membership valid.
    pub max_epochs: u32,
    /// Fewest tickets one purchase may buy.
    pub min_questions: u32,
    /// Launch share for the burn vault; once anyone has bought, `PURCHASE_BURN_BPS` rules instead.
    pub burn_bps: u16,
    /// Unused since v2; kept so the account layout does not change.
    pub slash_burn_bps: u16,
}

impl LaunchParams {
    fn check(&self) -> Result<()> {
        let p = self;
        require!(
            p.epoch_seconds >= MIN_EPOCH_SECONDS
                && p.rate > 0
                && p.limit > 0
                && p.max_epochs > 0
                && p.min_questions > 0
                && p.min_questions <= u32::from(p.limit)
                && p.rate.checked_mul(u64::from(p.limit)).is_some()
                && u64::from(p.burn_bps) <= BPS
                && u64::from(p.slash_burn_bps) <= BPS,
            ParanoidError::InvalidParameter
        );
        Ok(())
    }

    fn apply(&self, config: &mut Config, now: i64) {
        config.genesis = now;
        config.epoch_seconds = self.epoch_seconds;
        config.rate = self.rate;
        config.limit = self.limit;
        config.max_epochs = self.max_epochs;
        config.min_questions = self.min_questions;
        config.burn_bps = self.burn_bps;
        config.slash_burn_bps = self.slash_burn_bps;
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(constraint = program.programdata_address()? == Some(program_data.key()) @ ParanoidError::NotUpgradeAuthority)]
    pub program: Program<'info, Paranoid>,
    #[account(constraint = program_data.upgrade_authority_address == Some(authority.key()) @ ParanoidError::NotUpgradeAuthority)]
    pub program_data: Account<'info, ProgramData>,
    #[account(init, payer = authority, space = 8 + Config::INIT_SPACE, seeds = [CONFIG_SEED], bump)]
    pub config: Account<'info, Config>,
    #[account(init, payer = authority, space = 8 + std::mem::size_of::<Tree>(), seeds = [TREE_SEED], bump)]
    pub tree: AccountLoader<'info, Tree>,
    #[account(address = USDC_MINT)]
    pub usdc_mint: Account<'info, Mint>,
    #[account(init, payer = authority, seeds = [VAULT_SEED], bump, token::mint = usdc_mint, token::authority = config)]
    pub vault: Account<'info, TokenAccount>,
    #[account(init, payer = authority, seeds = [TREASURY_VAULT_SEED], bump, token::mint = usdc_mint, token::authority = config)]
    pub treasury_vault: Account<'info, TokenAccount>,
    #[account(init, payer = authority, seeds = [BURN_VAULT_SEED], bump, token::mint = usdc_mint, token::authority = config)]
    pub burn_vault: Account<'info, TokenAccount>,
    #[account(
        constraint = treasury.mint == USDC_MINT && treasury.owner != config.key() @ ParanoidError::InvalidTreasury,
    )]
    pub treasury: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

/// Upgrade authority, once: sets every launch constant. After the first member joins, nothing here
/// can be changed (see `reconfigure`).
pub fn initialize(ctx: Context<Initialize>, params: LaunchParams) -> Result<()> {
    let p = params;
    p.check()?;
    ctx.accounts.tree.load_init()?.init()?;
    let config = &mut ctx.accounts.config;
    config.set_inner(Config {
        bump: ctx.bumps.config,
        treasury: ctx.accounts.treasury.key(),
        genesis: Clock::get()?.unix_timestamp,
        epoch_seconds: p.epoch_seconds,
        rate: p.rate,
        limit: p.limit,
        max_epochs: p.max_epochs,
        min_questions: p.min_questions,
        burn_bps: p.burn_bps,
        slash_burn_bps: p.slash_burn_bps,
        burn_mint: Pubkey::default(),
        burn_pool: Pubkey::default(),
        members: 0,
        deposited: 0,
        paid_treasury: 0,
        paid_burn: 0,
        refunded: 0,
        swept: 0,
        burn_spent: 0,
        token_burned: 0,
    });
    Ok(())
}

#[derive(Accounts)]
pub struct SetBurnTarget<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(constraint = program.programdata_address()? == Some(program_data.key()) @ ParanoidError::NotUpgradeAuthority)]
    pub program: Program<'info, Paranoid>,
    #[account(constraint = program_data.upgrade_authority_address == Some(authority.key()) @ ParanoidError::NotUpgradeAuthority)]
    pub program_data: Account<'info, ProgramData>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    pub burn_mint: InterfaceAccount<'info, AnyMint>,
    #[account(address = WSOL_MINT)]
    pub wsol_mint: Account<'info, Mint>,
    /// CHECK: parsed by `read_damm_pool`, which checks the owner, the layout and the status.
    pub pool: UncheckedAccount<'info>,
    /// CHECK: parsed by `read_sol_usd`, which checks the owner, the feed and the verification level.
    pub price_update: UncheckedAccount<'info>,
    /// `init` makes this instruction once-only: a second call finds the account in use.
    #[account(
        init,
        payer = authority,
        seeds = [BOUGHT_SEED],
        bump,
        token::mint = burn_mint,
        token::authority = config,
        token::token_program = burn_token_program,
    )]
    pub bought: InterfaceAccount<'info, AnyTokenAccount>,
    /// Holds the SOL leg between `convert_burn` and `crank_burn`.
    #[account(
        init,
        payer = authority,
        seeds = [WSOL_VAULT_SEED],
        bump,
        token::mint = wsol_mint,
        token::authority = config,
    )]
    pub wsol_vault: Account<'info, TokenAccount>,
    #[account(init, payer = authority, space = 8 + BurnRoute::INIT_SPACE, seeds = [BURN_ROUTE_SEED], bump)]
    pub burn_route: Account<'info, BurnRoute>,
    pub token_program: Program<'info, Token>,
    pub burn_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

/// Upgrade authority, once, at token launch: names the token every burn buys and its Meteora
/// DAMM v2 pool against SOL. Until then purchases simply collect in the burn vault.
pub fn set_burn_target(ctx: Context<SetBurnTarget>) -> Result<()> {
    let pool = read_damm_pool(&ctx.accounts.pool)?;
    let mint = ctx.accounts.burn_mint.key();
    let pair_ok = (pool.mint_a == mint && pool.mint_b == WSOL_MINT) || (pool.mint_b == mint && pool.mint_a == WSOL_MINT);
    require!(pair_ok, ParanoidError::BurnPoolInvalid);
    read_sol_usd(&ctx.accounts.price_update, Clock::get()?.unix_timestamp, PRICE_MAX_AGE_SECS)?;
    let config = &mut ctx.accounts.config;
    config.burn_mint = mint;
    config.burn_pool = ctx.accounts.pool.key();
    ctx.accounts.burn_route.set_inner(BurnRoute { bump: ctx.bumps.burn_route, last_swap_slot: 0 });
    Ok(())
}

#[derive(Accounts)]
pub struct Reconfigure<'info> {
    pub authority: Signer<'info>,
    #[account(constraint = program.programdata_address()? == Some(program_data.key()) @ ParanoidError::NotUpgradeAuthority)]
    pub program: Program<'info, Paranoid>,
    #[account(constraint = program_data.upgrade_authority_address == Some(authority.key()) @ ParanoidError::NotUpgradeAuthority)]
    pub program_data: Account<'info, ProgramData>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
}

/// Upgrade authority, only while nobody has ever joined: replaces the launch constants and restarts
/// the day count. Once `members` is above zero the constants are fixed for good, since every member
/// bought under them.
pub fn reconfigure(ctx: Context<Reconfigure>, params: LaunchParams) -> Result<()> {
    params.check()?;
    let config = &mut ctx.accounts.config;
    require!(config.members == 0 && config.deposited == 0, ParanoidError::AlreadyInUse);
    params.apply(config, Clock::get()?.unix_timestamp);
    emit!(Reconfigured {
        epoch_seconds: config.epoch_seconds,
        rate: config.rate,
        limit: config.limit,
        max_epochs: config.max_epochs,
        min_questions: config.min_questions,
        burn_bps: config.burn_bps,
    });
    Ok(())
}
