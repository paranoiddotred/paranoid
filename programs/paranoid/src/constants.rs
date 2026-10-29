use anchor_lang::prelude::*;

pub const CONFIG_SEED: &[u8] = b"config";
pub const TREE_SEED: &[u8] = b"tree";
pub const VAULT_SEED: &[u8] = b"vault";
pub const TREASURY_VAULT_SEED: &[u8] = b"treasury_vault";
pub const BURN_VAULT_SEED: &[u8] = b"burn_vault";
pub const BOUGHT_SEED: &[u8] = b"bought";
pub const MEMBER_SEED: &[u8] = b"member";
pub const TOKEN_PAY_SEED: &[u8] = b"token_pay";
pub const ORDER_SEED: &[u8] = b"order";
pub const ORDER_VAULT_SEED: &[u8] = b"order_vault";
pub const STAKE_SEED: &[u8] = b"stake";
pub const STAKE_VAULT_SEED: &[u8] = b"stake_vault";
pub const STAKE_POSITION_SEED: &[u8] = b"stake_position";
pub const WALLET_STAKE_SEED: &[u8] = b"wallet_stake";
pub const STAKE_REWARD_SEED: &[u8] = b"stake_reward";
pub const STAKE_REWARD_VAULT_SEED: &[u8] = b"stake_reward_vault";

pub const BPS: u64 = 10_000;

/// 2^20 members over the vault's life. Must match `Request(20, 4)` in circuits/request.circom.
pub const TREE_DEPTH: usize = 20;
/// Recent roots a gateway may accept. Deposits and top-ups only add roots; a slash caps older ones.
pub const ROOT_HISTORY: usize = 64;
/// Tickets a single proof may spend. Must match `Request(20, 4)`.
pub const MAX_WEIGHT: u16 = 4;

pub const USDC_MINT: Pubkey = pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");

/// A burn swap must return at least this share of the pool's spot quote. The pool itself
/// keeps a 2% fee, so 97% leaves one point for rounding and drift.
pub const BURN_MIN_OUT_BPS: u64 = 9_700;
pub const BURN_ROUTE_SEED: &[u8] = b"burn_route";
pub const WSOL_VAULT_SEED: &[u8] = b"wsol";
/// Most burn-vault USDC one `convert_burn` call may sell.
pub const BURN_MAX_PER_CALL: u64 = 25_000_000;
/// The caller's pay for bringing SOL, taken off the oracle price.
pub const BURN_KEEPER_BPS: u64 = 30;
/// Slots a burn swap must wait after the one before it.
pub const BURN_COOLDOWN_SLOTS: u64 = 150;
/// Most of the pool's SOL side one burn swap may spend (bps).
pub const BURN_MAX_POOL_BPS: u64 = 20;

/// Mainnet epochs last at least an hour. Memberships are counted in epochs and a request hides among
/// the members valid in the epoch it names, so very short epochs would only shrink that set.
#[cfg(not(feature = "localnet"))]
pub const MIN_EPOCH_SECONDS: u32 = 3_600;
#[cfg(feature = "localnet")]
pub const MIN_EPOCH_SECONDS: u32 = 2;

/// Paying with the PARANOID token: the premium over the USDC price, or the discount under it, may be
/// at most 50%.
pub const MAX_PREMIUM_BPS: u16 = 5_000;
/// A SOL/USD update older than this cannot price a token payment.
#[cfg(not(feature = "localnet"))]
pub const PRICE_MAX_AGE_SECS: i64 = 60;
#[cfg(feature = "localnet")]
pub const PRICE_MAX_AGE_SECS: i64 = 30 * 86_400;

/// Most one gift card order may escrow: 1,000 USDC.
pub const ORDER_MAX: u64 = 1_000_000_000;

/// $PARANOID, the token that can be staked.
pub const PARANOID_MINT: Pubkey = pubkey!("22rWiyDiY9GgxSrFiWC2vX1GxNxjkvrRPmZJDzv9pq6X");
/// Days (epochs) between asking to unstake and taking the tokens back.
pub const UNSTAKE_EPOCHS: i64 = 3;
/// Share of every buyback handed to stakers (bps); the rest is burned. All of it burns while nobody stakes.
pub const STAKER_SHARE_BPS: u64 = 5_000;
/// Fixed-point scale of `StakeReward::per_token`.
pub const REWARD_SCALE: u128 = 1_000_000_000_000;
