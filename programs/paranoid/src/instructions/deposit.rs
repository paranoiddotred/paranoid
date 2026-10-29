use anchor_lang::prelude::*;
use anchor_spl::token::{Token, TokenAccount};

use crate::constants::*;
use crate::error::ParanoidError;
use crate::events::Deposited;
use crate::instructions::{check_questions, take_payment};
use crate::merkle::leaf;
use crate::state::{Config, Member, MemberStatus, Tree};

#[derive(Accounts)]
#[instruction(commitment: [u8; 32])]
pub struct Deposit<'info> {
    #[account(mut)]
    pub depositor: Signer<'info>,
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(mut, seeds = [TREE_SEED], bump)]
    pub tree: AccountLoader<'info, Tree>,
    #[account(
        init,
        payer = depositor,
        space = 8 + Member::INIT_SPACE,
        seeds = [MEMBER_SEED, commitment.as_ref()],
        bump,
    )]
    pub member: Account<'info, Member>,
    #[account(mut, token::mint = USDC_MINT, token::authority = depositor)]
    pub source: Account<'info, TokenAccount>,
    #[account(mut, seeds = [TREASURY_VAULT_SEED], bump)]
    pub treasury_vault: Account<'info, TokenAccount>,
    #[account(mut, seeds = [BURN_VAULT_SEED], bump)]
    pub burn_vault: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

/// Buys `questions` tickets for a new membership valid for `max_epochs` days from today, and adds
/// the leaf Poseidon(commitment, end_epoch, questions) to the tree. The secret behind `commitment`
/// never leaves the client; the buyer's wallet is public, which links it to the commitment and to
/// how many tickets it bought, but to no request.
pub fn deposit(mut ctx: Context<Deposit>, commitment: [u8; 32], questions: u32) -> Result<()> {
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
    let amount = take_payment(&mut a.config, questions, &a.depositor, &a.source, &a.treasury_vault, &a.burn_vault, &a.token_program)?;
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
    emit!(Deposited { commitment, leaf_index, start_epoch, end_epoch, questions, amount, root });
    Ok(())
}
