use anchor_lang::prelude::*;

use crate::constants::{ROOT_HISTORY, TREE_DEPTH};
use crate::error::ParanoidError;

/// Launch constants and running totals. Every amount is in USDC base units (6 decimals).
///
/// v2 (question packs) keeps v1's byte layout so the live account can be reconfigured in place;
/// fields whose meaning changed are renamed and documented below.
#[account]
#[derive(InitSpace)]
pub struct Config {
    pub bump: u8,
    /// USDC token account the treasury vault is swept to. Purchases never send to it directly, so closing
    /// or freezing it cannot block anyone; since 2026-10-27 they send the vault nothing (`PURCHASE_BURN_BPS`).
    pub treasury: Pubkey,
    /// Unix time at which day 0 began.
    pub genesis: i64,
    /// Length of one day (epoch). Keys and validity are counted in these.
    pub epoch_seconds: u32,
    /// Price of one question (one ticket).
    pub rate: u64,
    /// Most tickets one membership may hold (the circuit's `limit`, below 2^16).
    pub limit: u16,
    /// Days a purchase or top-up keeps the membership valid.
    pub max_epochs: u32,
    /// Fewest tickets one purchase or top-up may buy.
    pub min_questions: u32,
    /// Launch share for the burn vault, frozen at 2,000 by the first purchase; `PURCHASE_BURN_BPS` rules.
    pub burn_bps: u16,
    /// Unused since v2 (nothing is held for members, so a slash moves no money). Kept for layout.
    pub slash_burn_bps: u16,
    /// The token the burn crank buys and burns, and its Meteora DAMM v2 pool against SOL. Unset before launch.
    pub burn_mint: Pubkey,
    pub burn_pool: Pubkey,
    pub members: u64,
    pub deposited: u64,
    pub paid_treasury: u64,
    pub paid_burn: u64,
    /// Always 0 since v2: purchases are not refundable. Kept for layout.
    pub refunded: u64,
    pub swept: u64,
    pub burn_spent: u64,
    pub token_burned: u64,
}

impl Config {
    pub fn epoch_at(&self, unix_timestamp: i64) -> Result<u32> {
        let elapsed = unix_timestamp.checked_sub(self.genesis).ok_or(ParanoidError::MathOverflow)?;
        let epoch = elapsed.max(0) / i64::from(self.epoch_seconds);
        u32::try_from(epoch).map_err(|_| error!(ParanoidError::MathOverflow))
    }

    pub fn current_epoch(&self) -> Result<u32> {
        self.epoch_at(Clock::get()?.unix_timestamp)
    }

    /// Last valid day of a purchase made on day `now`.
    pub fn valid_through(&self, now: u32) -> Result<u32> {
        now.checked_add(self.max_epochs - 1).ok_or(error!(ParanoidError::MathOverflow))
    }
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug, InitSpace)]
pub enum MemberStatus {
    /// Leaf in the tree; may spend tickets through `end_epoch`.
    Active,
    /// Secret revealed by a ticket used twice. Kept as a tombstone so the commitment can never rejoin.
    Slashed,
}

/// One membership, at the PDA ["member", commitment]: a commitment can be in the tree only once.
#[account]
#[derive(InitSpace)]
pub struct Member {
    pub bump: u8,
    pub depositor: Pubkey,
    /// Poseidon(secret). The leaf is Poseidon(commitment, end_epoch, questions).
    pub commitment: [u8; 32],
    pub leaf_index: u32,
    pub start_epoch: u32,
    /// Last day the member's tickets are accepted. Every top-up moves it to a full `max_epochs` again.
    pub end_epoch: u32,
    /// Tickets bought so far, numbered 0 .. questions - 1. Tickets are spent off-chain; the chain
    /// never learns how many were used.
    pub questions: u64,
    pub status: MemberStatus,
}

#[zero_copy]
pub struct RootEntry {
    pub root: [u8; 32],
    /// A gateway accepts this root for epoch `e` only while `e < valid_before`.
    pub valid_before: u32,
}

/// Append-only Poseidon Merkle tree of member leaves with an editable frontier, so a leaf can be
/// swapped on a top-up or zeroed on a slash and later insertions still extend the edited tree.
#[account(zero_copy)]
pub struct Tree {
    pub next_index: u32,
    /// Slot of the newest entry in `roots`.
    pub head: u32,
    /// zeros[i] is the root of an empty subtree of height i; zeros[0] = 0 is the empty leaf.
    pub zeros: [[u8; 32]; TREE_DEPTH + 1],
    /// filled[i] is the left node at height i on the path of the most recent insertion.
    pub filled: [[u8; 32]; TREE_DEPTH],
    pub roots: [RootEntry; ROOT_HISTORY],
}

/// Paying for questions with the PARANOID token, set once by the upgrade authority. Every token paid
/// is burned on the spot; no USDC moves, so none of the USDC totals in `Config` change.
#[account]
#[derive(InitSpace)]
pub struct TokenPay {
    pub bump: u8,
    pub mint: Pubkey,
    /// The token's Meteora DAMM v2 pool against SOL.
    pub pool: Pubkey,
    /// Pyth's SOL/USD price update account.
    pub price_update: Pubkey,
    /// Paid in tokens on top of the USDC price; negative takes that much off it.
    pub premium_bps: i16,
    pub questions_sold: u64,
    pub tokens_burned: u64,
}

/// Created once by `set_burn_target`: the burn swap's cooldown.
#[account]
#[derive(InitSpace)]
pub struct BurnRoute {
    pub bump: u8,
    pub last_swap_slot: u64,
}

/// One gift card order's escrow, opened by `open_order` and closed by `settle_order`.
#[account]
#[derive(InitSpace)]
pub struct Order {
    pub bump: u8,
    pub buyer: Pubkey,
    pub key: Pubkey,
    pub amount: u64,
}

/// All staked $PARANOID. Its PDA owns the vault the tokens sit in.
#[account]
#[derive(InitSpace)]
pub struct Stake {
    pub bump: u8,
    /// Tokens staked by open positions.
    pub staked: u64,
    pub positions: u64,
    /// Questions handed out to stakers so far.
    pub questions_given: u64,
}

/// A stake from before 2026-10-27: it sat on a membership and earned questions. None are opened any more;
/// `return_legacy_stake` gives the tokens back.
#[account]
#[derive(InitSpace)]
pub struct StakePosition {
    pub bump: u8,
    pub member: Pubkey,
    /// Tokens staked.
    pub amount: u64,
    /// Unix time up to which `earned` is counted.
    pub since: i64,
    /// Token-seconds earned and not yet turned into questions.
    pub earned: u128,
    /// 0 while staked. Once the depositor asks to unstake: the unix time the tokens can be taken back.
    /// Nothing is earned from the request on.
    pub unstake_at: i64,
}

/// A wallet's stake: $PARANOID held in the program that shares every buyback while it is not unstaking.
#[account]
#[derive(InitSpace)]
pub struct WalletStake {
    pub bump: u8,
    pub owner: Pubkey,
    /// Tokens staked.
    pub amount: u64,
    /// 0 while staked. Once the owner asks to unstake: the unix time the tokens can be taken back.
    /// Nothing is earned from the request on.
    pub unstake_at: i64,
    /// `StakeReward::per_token` when this stake was last settled.
    pub per_token_seen: u128,
    /// Bought $PARANOID this stake has earned and not yet claimed.
    pub tokens_owed: u64,
}

/// The tokens the burn crank buys with purchase fees, shared among stakes that are not unstaking.
#[account]
#[derive(InitSpace)]
pub struct StakeReward {
    pub bump: u8,
    /// Tokens each staked base unit has earned since the start, times `REWARD_SCALE`.
    pub per_token: u128,
    /// Tokens in wallet stakes that are not unstaking: the ones that share each buy.
    pub earning: u64,
    /// Tokens handed to stakers so far.
    pub given: u64,
    /// Tokens stakers have claimed so far.
    pub claimed: u64,
}
