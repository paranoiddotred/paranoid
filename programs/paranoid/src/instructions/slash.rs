use anchor_lang::prelude::*;

use crate::constants::*;
use crate::error::ParanoidError;
use crate::events::Slashed;
use crate::merkle::{leaf, poseidon};
use crate::state::{Config, Member, MemberStatus, Tree};

#[derive(Accounts)]
pub struct Slash<'info> {
    pub reporter: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(mut, seeds = [TREE_SEED], bump)]
    pub tree: AccountLoader<'info, Tree>,
    #[account(mut, seeds = [MEMBER_SEED, member.commitment.as_ref()], bump = member.bump)]
    pub member: Account<'info, Member>,
}

/// Anyone holding a member's secret: the only way a secret leaves a client is by spending one
/// ticket twice, which puts two points of the same line in front of the gateway. The leaf leaves
/// the tree, so every ticket the member still had is gone; the money was already paid at purchase.
/// The reporter gets nothing, so there is nothing to front-run. Roots that contained the leaf stop
/// being accepted from today, since anyone can now prove with the public secret.
pub fn slash(ctx: Context<Slash>, secret: [u8; 32], siblings: Vec<[u8; 32]>) -> Result<()> {
    let member = &mut ctx.accounts.member;
    require!(member.status != MemberStatus::Slashed, ParanoidError::AlreadySlashed);
    require!(poseidon(&[&secret])? == member.commitment, ParanoidError::SecretMismatch);
    let now = ctx.accounts.config.current_epoch()?;

    let mut tree = ctx.accounts.tree.load_mut()?;
    let old_leaf = leaf(&member.commitment, member.end_epoch, member.questions)?;
    let root = tree.replace(member.leaf_index, old_leaf, &siblings, [0u8; 32])?;
    tree.cap_roots(now);
    tree.push_root(root);
    drop(tree);

    member.status = MemberStatus::Slashed;
    emit!(Slashed { commitment: member.commitment, questions_lost: member.questions });
    Ok(())
}
