//! Paying for questions with the PARANOID token. The price is the USDC price of the questions plus
//! a premium (or minus a discount), converted at the token's Meteora DAMM v2 pool price against SOL
//! times Pyth's SOL/USD. Every token paid is burned in the same instruction. No USDC moves.

use anchor_lang::prelude::*;
use anchor_spl::token::{self, Burn, Mint, Token, TokenAccount};

use crate::constants::*;
use crate::error::ParanoidError;
use crate::events::{Deposited, PaidInToken, ToppedUp};
use crate::instructions::check_questions;
use crate::math::price;
use crate::merkle::leaf;
use crate::meteora::{read_damm_pool, read_sol_usd, token_cost, WSOL_MINT};
use crate::program::Paranoid;
use crate::state::{Config, Member, MemberStatus, TokenPay, Tree};

#[derive(Accounts)]
pub struct SetTokenPayment<'info> {
    #[account(mut)]
    pub authority: Signer<'info>,
    #[account(constraint = program.programdata_address()? == Some(program_data.key()) @ ParanoidError::NotUpgradeAuthority)]
    pub program: Program<'info, Paranoid>,
    #[account(constraint = program_data.upgrade_authority_address == Some(authority.key()) @ ParanoidError::NotUpgradeAuthority)]
    pub program_data: Account<'info, ProgramData>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    pub mint: Account<'info, Mint>,
    /// CHECK: parsed by `read_damm_pool`, which checks the owner, the layout and the status.
    pub pool: UncheckedAccount<'info>,
    /// CHECK: parsed by `read_sol_usd`, which checks the owner, the feed and the verification level.
    pub price_update: UncheckedAccount<'info>,
    /// `init` makes this once-only: a second call finds the account in use.
    #[account(init, payer = authority, space = 8 + TokenPay::INIT_SPACE, seeds = [TOKEN_PAY_SEED], bump)]
    pub token_pay: Account<'info, TokenPay>,
    pub system_program: Program<'info, System>,
}

/// Upgrade authority, once: names the token, its SOL pool, the SOL/USD price account and the premium.
pub fn set_token_payment(ctx: Context<SetTokenPayment>, premium_bps: i16) -> Result<()> {
    require!(premium_bps.unsigned_abs() <= MAX_PREMIUM_BPS, ParanoidError::InvalidParameter);
    let pool = read_damm_pool(&ctx.accounts.pool)?;
    let mint = ctx.accounts.mint.key();
    let pair_ok = (pool.mint_a == mint && pool.mint_b == WSOL_MINT) || (pool.mint_b == mint && pool.mint_a == WSOL_MINT);
    require!(pair_ok, ParanoidError::TokenPayInvalid);
    read_sol_usd(&ctx.accounts.price_update, Clock::get()?.unix_timestamp, PRICE_MAX_AGE_SECS)?;
    ctx.accounts.token_pay.set_inner(TokenPay {
        bump: ctx.bumps.token_pay,
        mint,
        pool: ctx.accounts.pool.key(),
        price_update: ctx.accounts.price_update.key(),
        premium_bps,
        questions_sold: 0,
        tokens_burned: 0,
    });
    Ok(())
}

#[derive(Accounts)]
pub struct SetTokenPremium<'info> {
    pub authority: Signer<'info>,
    #[account(constraint = program.programdata_address()? == Some(program_data.key()) @ ParanoidError::NotUpgradeAuthority)]
    pub program: Program<'info, Paranoid>,
    #[account(constraint = program_data.upgrade_authority_address == Some(authority.key()) @ ParanoidError::NotUpgradeAuthority)]
    pub program_data: Account<'info, ProgramData>,
    #[account(mut, seeds = [TOKEN_PAY_SEED], bump = token_pay.bump)]
    pub token_pay: Account<'info, TokenPay>,
}

/// Upgrade authority, any time: moves the premium; negative makes it a discount.
pub fn set_token_premium(ctx: Context<SetTokenPremium>, premium_bps: i16) -> Result<()> {
    require!(premium_bps.unsigned_abs() <= MAX_PREMIUM_BPS, ParanoidError::InvalidParameter);
    ctx.accounts.token_pay.premium_bps = premium_bps;
    Ok(())
}

/// Prices `questions` in tokens, checks it against `max_tokens`, burns it from `source`.
#[allow(clippy::too_many_arguments)]
fn burn_payment<'info>(
    config: &Config,
    token_pay: &mut Account<'info, TokenPay>,
    questions: u64,
    max_tokens: u64,
    pool: &AccountInfo<'info>,
    price_update: &AccountInfo<'info>,
    mint: &Account<'info, Mint>,
    source: &Account<'info, TokenAccount>,
    buyer: &Signer<'info>,
    token_program: &Program<'info, Token>,
) -> Result<(u64, u64)> {
    let usdc_price = price(config.rate, questions).ok_or(ParanoidError::MathOverflow)?;
    let p = read_damm_pool(pool)?;
    let (sol_usd, expo) = read_sol_usd(price_update, Clock::get()?.unix_timestamp, PRICE_MAX_AGE_SECS)?;
    let tokens = token_cost(usdc_price, token_pay.premium_bps, p.sqrt_price, p.mint_a == token_pay.mint, sol_usd, expo)
        .ok_or(ParanoidError::MathOverflow)?;
    require!(tokens <= max_tokens, ParanoidError::CostAboveMax);
    token::burn(
        CpiContext::new(
            token_program.to_account_info(),
            Burn { mint: mint.to_account_info(), from: source.to_account_info(), authority: buyer.to_account_info() },
        ),
        tokens,
    )?;
    token_pay.questions_sold = token_pay.questions_sold.checked_add(questions).ok_or(ParanoidError::MathOverflow)?;
    token_pay.tokens_burned = token_pay.tokens_burned.checked_add(tokens).ok_or(ParanoidError::MathOverflow)?;
    Ok((usdc_price, tokens))
}

#[derive(Accounts)]
#[instruction(commitment: [u8; 32])]
pub struct DepositToken<'info> {
    #[account(mut)]
    pub depositor: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(mut, seeds = [TREE_SEED], bump)]
    pub tree: AccountLoader<'info, Tree>,
    #[account(init, payer = depositor, space = 8 + Member::INIT_SPACE, seeds = [MEMBER_SEED, commitment.as_ref()], bump)]
    pub member: Account<'info, Member>,
    #[account(mut, seeds = [TOKEN_PAY_SEED], bump = token_pay.bump)]
    pub token_pay: Account<'info, TokenPay>,
    #[account(mut, address = token_pay.mint @ ParanoidError::TokenPayInvalid)]
    pub mint: Account<'info, Mint>,
    #[account(mut, token::mint = mint, token::authority = depositor)]
    pub source: Account<'info, TokenAccount>,
    /// CHECK: must be the configured pool; parsed by `read_damm_pool`.
    #[account(address = token_pay.pool @ ParanoidError::TokenPayInvalid)]
    pub pool: UncheckedAccount<'info>,
    /// CHECK: must be the configured price account; parsed by `read_sol_usd`.
    #[account(address = token_pay.price_update @ ParanoidError::PriceInvalid)]
    pub price_update: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

/// `deposit`, paid in tokens: a new membership with `questions` tickets. Burns at most `max_tokens`.
pub fn deposit_token(mut ctx: Context<DepositToken>, commitment: [u8; 32], questions: u32, max_tokens: u64) -> Result<()> {
    require!(commitment != [0u8; 32], ParanoidError::InvalidCommitment);
    let questions = u64::from(questions);
    check_questions(&ctx.accounts.config, questions, questions)?;
    let start_epoch = ctx.accounts.config.current_epoch()?;
    let end_epoch = ctx.accounts.config.valid_through(start_epoch)?;
    let new_leaf = leaf(&commitment, end_epoch, questions).map_err(|_| error!(ParanoidError::InvalidCommitment))?;

    let mut tree = ctx.accounts.tree.load_mut()?;
    let leaf_index = tree.insert(new_leaf)?;
    let root = tree.root();
    drop(tree);

    let a = &mut ctx.accounts;
    let (usdc_price, tokens) = burn_payment(
        &a.config, &mut a.token_pay, questions, max_tokens, &a.pool, &a.price_update, &a.mint, &a.source, &a.depositor, &a.token_program,
    )?;
    a.member.set_inner(Member {
        bump: ctx.bumps.member,
        depositor: a.depositor.key(),
        commitment,
        leaf_index,
        start_epoch,
        end_epoch,
        questions,
        status: MemberStatus::Active,
    });
    a.config.members = a.config.members.checked_add(1).ok_or(ParanoidError::MathOverflow)?;
    emit!(Deposited { commitment, leaf_index, start_epoch, end_epoch, questions, amount: 0, root });
    emit!(PaidInToken { commitment, questions, usdc_price, tokens_burned: tokens });
    Ok(())
}

#[derive(Accounts)]
pub struct TopUpToken<'info> {
    #[account(mut)]
    pub depositor: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(mut, seeds = [TREE_SEED], bump)]
    pub tree: AccountLoader<'info, Tree>,
    #[account(
        mut,
        seeds = [MEMBER_SEED, member.commitment.as_ref()],
        bump = member.bump,
        has_one = depositor @ ParanoidError::NotDepositor,
    )]
    pub member: Account<'info, Member>,
    #[account(mut, seeds = [TOKEN_PAY_SEED], bump = token_pay.bump)]
    pub token_pay: Account<'info, TokenPay>,
    #[account(mut, address = token_pay.mint @ ParanoidError::TokenPayInvalid)]
    pub mint: Account<'info, Mint>,
    #[account(mut, token::mint = mint, token::authority = depositor)]
    pub source: Account<'info, TokenAccount>,
    /// CHECK: must be the configured pool; parsed by `read_damm_pool`.
    #[account(address = token_pay.pool @ ParanoidError::TokenPayInvalid)]
    pub pool: UncheckedAccount<'info>,
    /// CHECK: must be the configured price account; parsed by `read_sol_usd`.
    #[account(address = token_pay.price_update @ ParanoidError::PriceInvalid)]
    pub price_update: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
}

/// `top_up`, paid in tokens. Burns at most `max_tokens`.
pub fn top_up_token(mut ctx: Context<TopUpToken>, questions: u32, siblings: Vec<[u8; 32]>, max_tokens: u64) -> Result<()> {
    let member = &ctx.accounts.member;
    require!(member.status == MemberStatus::Active, ParanoidError::NotActive);
    let bought = u64::from(questions);
    let total = member.questions.checked_add(bought).ok_or(ParanoidError::MathOverflow)?;
    check_questions(&ctx.accounts.config, bought, total)?;
    let now = ctx.accounts.config.current_epoch()?;
    let end_epoch = ctx.accounts.config.valid_through(now)?;

    let old_leaf = leaf(&member.commitment, member.end_epoch, member.questions)?;
    let new_leaf = leaf(&member.commitment, end_epoch, total)?;
    let mut tree = ctx.accounts.tree.load_mut()?;
    let root = tree.replace(member.leaf_index, old_leaf, &siblings, new_leaf)?;
    tree.push_root(root);
    drop(tree);

    let a = &mut ctx.accounts;
    let (usdc_price, tokens) = burn_payment(
        &a.config, &mut a.token_pay, bought, max_tokens, &a.pool, &a.price_update, &a.mint, &a.source, &a.depositor, &a.token_program,
    )?;
    a.member.questions = total;
    a.member.end_epoch = end_epoch;
    emit!(ToppedUp { commitment: a.member.commitment, questions: total, end_epoch, amount: 0, root });
    emit!(PaidInToken { commitment: a.member.commitment, questions: bought, usdc_price, tokens_burned: tokens });
    Ok(())
}
