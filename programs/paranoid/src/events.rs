use anchor_lang::prelude::*;

#[event]
pub struct Deposited {
    pub commitment: [u8; 32],
    pub leaf_index: u32,
    pub start_epoch: u32,
    pub end_epoch: u32,
    pub questions: u64,
    pub amount: u64,
    pub root: [u8; 32],
}

#[event]
pub struct ToppedUp {
    pub commitment: [u8; 32],
    /// Tickets the membership now holds in total.
    pub questions: u64,
    pub end_epoch: u32,
    pub amount: u64,
    pub root: [u8; 32],
}

#[event]
pub struct Slashed {
    pub commitment: [u8; 32],
    pub questions_lost: u64,
}

#[event]
pub struct Reconfigured {
    pub epoch_seconds: u32,
    pub rate: u64,
    pub limit: u16,
    pub max_epochs: u32,
    pub min_questions: u32,
    pub burn_bps: u16,
}

#[event]
pub struct Swept {
    pub amount: u64,
}

#[event]
pub struct BurnFunded {
    /// USDC sold out of the burn vault.
    pub usdc: u64,
    /// wSOL received for it at the oracle price, less the caller's pay.
    pub lamports: u64,
}

#[event]
pub struct Burned {
    pub lamports_spent: u64,
    pub token_burned: u64,
}

#[event]
pub struct PaidInToken {
    pub commitment: [u8; 32],
    /// Tickets bought in this purchase.
    pub questions: u64,
    /// The USDC price those tickets would have cost.
    pub usdc_price: u64,
    /// PARANOID base units burned for them.
    pub tokens_burned: u64,
}

#[event]
pub struct StakeAdded {
    pub owner: Pubkey,
    pub amount: u64,
    /// Tokens the stake holds after this one.
    pub staked: u64,
}

#[event]
pub struct UnstakeRequested {
    pub owner: Pubkey,
    pub amount: u64,
    pub unstake_at: i64,
}

#[event]
pub struct StakeWithdrawn {
    pub owner: Pubkey,
    pub amount: u64,
}

#[event]
pub struct StakersPaid {
    pub lamports_spent: u64,
    pub tokens: u64,
    pub earning: u64,
}

#[event]
pub struct StakeRewardClaimed {
    pub owner: Pubkey,
    pub tokens: u64,
}

#[event]
pub struct LegacyStakeReturned {
    pub commitment: [u8; 32],
    pub amount: u64,
}
