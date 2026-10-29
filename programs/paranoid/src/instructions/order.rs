use anchor_lang::prelude::*;
use anchor_spl::token::{self, CloseAccount, Mint, Token, TokenAccount, Transfer};

use crate::constants::*;
use crate::error::ParanoidError;
use crate::state::Order;

/// Escrow for one gift card order. The buyer's wallet pays into a vault this program owns (the same
/// kind of payment as buying questions), and the order names the throwaway key the buyer's browser
/// made. Only that key can send the money on (to pay the store, or back), and the buyer can always
/// cancel and take it back. Nothing else can move it, and the program keeps none of it.
#[derive(Accounts)]
pub struct OpenOrder<'info> {
    #[account(mut)]
    pub buyer: Signer<'info>,
    /// CHECK: the throwaway key; only its signature (or the buyer's) can empty the order.
    pub key: UncheckedAccount<'info>,
    #[account(init, payer = buyer, space = 8 + Order::INIT_SPACE, seeds = [ORDER_SEED, key.key().as_ref()], bump)]
    pub order: Account<'info, Order>,
    #[account(
        init,
        payer = buyer,
        seeds = [ORDER_VAULT_SEED, key.key().as_ref()],
        bump,
        token::mint = usdc_mint,
        token::authority = order,
    )]
    pub vault: Account<'info, TokenAccount>,
    #[account(mut, token::mint = USDC_MINT, token::authority = buyer)]
    pub source: Account<'info, TokenAccount>,
    #[account(address = USDC_MINT)]
    pub usdc_mint: Account<'info, Mint>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

pub fn open_order(mut ctx: Context<OpenOrder>, amount: u64) -> Result<()> {
    require!(amount > 0 && amount <= ORDER_MAX, ParanoidError::OrderAmountInvalid);
    let a = &mut ctx.accounts;
    token::transfer(
        CpiContext::new(
            a.token_program.to_account_info(),
            Transfer { from: a.source.to_account_info(), to: a.vault.to_account_info(), authority: a.buyer.to_account_info() },
        ),
        amount,
    )?;
    a.order.set_inner(Order { bump: ctx.bumps.order, buyer: a.buyer.key(), key: a.key.key(), amount });
    Ok(())
}

/// Empties an order into `destination` and closes it; the account deposits go back to the buyer.
/// The key may send the money anywhere (it pays the store from its own account); the buyer may only
/// send it to a USDC account of their own.
#[derive(Accounts)]
pub struct SettleOrder<'info> {
    pub signer: Signer<'info>,
    #[account(mut, seeds = [ORDER_SEED, order.key.as_ref()], bump = order.bump, has_one = buyer, close = buyer)]
    pub order: Account<'info, Order>,
    /// CHECK: receives the deposits of the closed accounts; must be the order's buyer.
    #[account(mut)]
    pub buyer: UncheckedAccount<'info>,
    #[account(mut, seeds = [ORDER_VAULT_SEED, order.key.as_ref()], bump)]
    pub vault: Account<'info, TokenAccount>,
    #[account(mut, token::mint = USDC_MINT)]
    pub destination: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

pub fn settle_order(ctx: Context<SettleOrder>) -> Result<()> {
    let a = &ctx.accounts;
    let by_key = a.signer.key() == a.order.key;
    let by_buyer = a.signer.key() == a.order.buyer && a.destination.owner == a.order.buyer;
    require!(by_key || by_buyer, ParanoidError::NotOrderParty);
    let key = a.order.key;
    let seeds: &[&[u8]] = &[ORDER_SEED, key.as_ref(), &[a.order.bump]];
    let amount = a.vault.amount;
    if amount > 0 {
        token::transfer(
            CpiContext::new_with_signer(
                a.token_program.to_account_info(),
                Transfer { from: a.vault.to_account_info(), to: a.destination.to_account_info(), authority: a.order.to_account_info() },
                &[seeds],
            ),
            amount,
        )?;
    }
    token::close_account(CpiContext::new_with_signer(
        a.token_program.to_account_info(),
        CloseAccount { account: a.vault.to_account_info(), destination: a.buyer.to_account_info(), authority: a.order.to_account_info() },
        &[seeds],
    ))
}
