use anchor_lang::prelude::*;

#[error_code]
pub enum ParanoidError {
    #[msg("Only the program's upgrade authority may initialize or set the burn target")]
    NotUpgradeAuthority,
    #[msg("A launch parameter is out of range")]
    InvalidParameter,
    #[msg("The treasury must be a USDC token account")]
    InvalidTreasury,
    /// Kept so the codes after it stay put; nothing raises it since v2 (question packs).
    #[msg("Subscription length must be between 1 and max_epochs")]
    InvalidEpochs,
    #[msg("The identity commitment must be a nonzero BN254 field element")]
    InvalidCommitment,
    #[msg("Value is not a BN254 field element")]
    NotAFieldElement,
    #[msg("The member tree is full")]
    TreeFull,
    #[msg("The Merkle path does not lead to the current root")]
    InvalidPath,
    /// Kept so the codes after it stay put; nothing raises it since v2 (question packs).
    #[msg("The membership has already ended; withdraw after the slashing window instead")]
    MembershipEnded,
    #[msg("Only the depositor may do this")]
    NotDepositor,
    #[msg("The member is not active")]
    NotActive,
    /// Kept so the codes after it stay put; nothing raises it since v2 (question packs).
    #[msg("The member is not exiting, or its slashing window has not passed")]
    NotWithdrawable,
    #[msg("The member was already slashed")]
    AlreadySlashed,
    #[msg("The revealed secret does not open this member's commitment")]
    SecretMismatch,
    /// The message predates the move to Meteora; raised when the pool, its vaults or the mint are not the
    /// configured Meteora DAMM v2 pool against SOL. Messages are compiled in, so it changes with an upgrade.
    #[msg("The pool is not the configured Raydium CP-Swap USDC pool of the burn mint")]
    BurnPoolInvalid,
    /// Kept so the codes after it stay put; nothing raises it since v2 (question packs).
    #[msg("The pool has no observation old enough to price the buy")]
    BurnTwapUnavailable,
    /// The message predates the move to Meteora; the caps are now `BURN_MAX_PER_CALL` per conversion and
    /// `BURN_MAX_POOL_BPS` of the pool's SOL side per swap.
    #[msg("Burn amount must be positive, funded, and within 0.1% of the pool's USDC side")]
    BurnAmountInvalid,
    #[msg("Arithmetic overflow")]
    MathOverflow,
    #[msg("Buy at least the minimum number of questions, and keep the membership under its cap")]
    InvalidQuestions,
    #[msg("Launch constants can change only before anyone has joined")]
    AlreadyInUse,
    #[msg("The pool or token is not the configured PARANOID payment pool")]
    TokenPayInvalid,
    #[msg("The SOL/USD price update is not a verified Pyth SOL/USD update")]
    PriceInvalid,
    #[msg("The SOL/USD price is too old; post a fresh Pyth update and retry")]
    PriceStale,
    #[msg("The token price moved past the most you agreed to pay")]
    CostAboveMax,
    #[msg("The burn swap ran a moment ago; wait a few slots")]
    BurnTooSoon,
    #[msg("An order must hold more than nothing and at most 1,000 USDC")]
    OrderAmountInvalid,
    #[msg("Only the order's key, or its buyer sending the money back to themselves, can settle it")]
    NotOrderParty,
    #[msg("Stake more than 0 $PARANOID")]
    StakeTooSmall,
    /// Kept so the codes after it stay put; nothing raises it since stakes stopped earning questions.
    #[msg("Not enough earned yet for one question")]
    NothingToHarvest,
    #[msg("This stake is being unstaked")]
    Unstaking,
    #[msg("Ask to unstake first, then wait 3 days")]
    UnstakeNotReady,
    #[msg("No $PARANOID earned yet to claim")]
    NothingToClaim,
}
