use anchor_lang::prelude::*;
use anchor_spl::token::{Token, TokenAccount};

use crate::constants::*;
use crate::error::ParanoidError;
use crate::events::Swept;
use crate::instructions::pay_from;
use crate::state::Config;

#[derive(Accounts)]
pub struct SweepTreasury<'info> {
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump, has_one = treasury)]
    pub config: Account<'info, Config>,
    #[account(mut, seeds = [TREASURY_VAULT_SEED], bump)]
    pub treasury_vault: Account<'info, TokenAccount>,
    #[account(mut)]
    pub treasury: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

/// Anyone: moves everything billed to the treasury vault on to the launch-fixed treasury account.
pub fn sweep_treasury(ctx: Context<SweepTreasury>) -> Result<()> {
    let amount = ctx.accounts.treasury_vault.amount;
    let treasury = ctx.accounts.treasury.to_account_info();
    pay_from(&ctx.accounts.token_program, &ctx.accounts.treasury_vault, &treasury, &ctx.accounts.config, amount)?;
    let config = &mut ctx.accounts.config;
    config.swept = config.swept.checked_add(amount).ok_or(ParanoidError::MathOverflow)?;
    emit!(Swept { amount });
    Ok(())
}
