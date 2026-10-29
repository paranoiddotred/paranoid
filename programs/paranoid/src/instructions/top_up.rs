use anchor_lang::prelude::*;
use anchor_spl::token::{Token, TokenAccount};

use crate::constants::*;
use crate::error::ParanoidError;
use crate::events::ToppedUp;
use crate::instructions::{check_questions, take_payment};
use crate::merkle::leaf;
use crate::state::{Config, Member, MemberStatus, Tree};

#[derive(Accounts)]
pub struct TopUp<'info> {
    #[account(mut)]
    pub depositor: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
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
    #[account(mut, token::mint = USDC_MINT, token::authority = depositor)]
    pub source: Account<'info, TokenAccount>,
    #[account(mut, seeds = [TREASURY_VAULT_SEED], bump)]
    pub treasury_vault: Account<'info, TokenAccount>,
    #[account(mut, seeds = [BURN_VAULT_SEED], bump)]
    pub burn_vault: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

/// Depositor: buys `questions` more tickets for the same membership, numbered after the ones it
/// already has, and makes it valid for a full `max_epochs` days again. The leaf is swapped for one
/// with the new count and day; roots that hold the old leaf stay accepted, since the old leaf only
/// proves fewer tickets of the same secret.
pub fn top_up(mut ctx: Context<TopUp>, questions: u32, siblings: Vec<[u8; 32]>) -> Result<()> {
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
    let amount = take_payment(&mut a.config, bought, &a.depositor, &a.source, &a.treasury_vault, &a.burn_vault, &a.token_program)?;
    a.member.questions = total;
    a.member.end_epoch = end_epoch;
    emit!(ToppedUp { commitment: a.member.commitment, questions: total, end_epoch, amount, root });
    Ok(())
}
