//! Pricing for paying with the PARANOID token: the token's Meteora DAMM v2 pool against SOL, times
//! Pyth's SOL/USD. Pure helpers (`token_cost`, `mul_div_ceil`) are property-tested on the host.

use anchor_lang::prelude::*;
use anchor_lang::solana_program::instruction::{AccountMeta, Instruction};
use anchor_lang::solana_program::program::invoke_signed;

use crate::constants::BPS;
use crate::error::ParanoidError;

pub const DAMM_V2: Pubkey = pubkey!("cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG");
/// DAMM v2's fixed pool authority and its Anchor event authority PDA.
pub const POOL_AUTHORITY: Pubkey = pubkey!("HLnpSz9h2S4hiLQ43rnSD9XkcUThA7B8hQMKmDaiTLcC");
pub const EVENT_AUTHORITY: Pubkey = pubkey!("3rmHSu74h1ZcmAisVcWerTCiRDQbUrBKmcwptYGjHfet");
pub const PYTH_RECEIVER: Pubkey = pubkey!("rec5EKMGg6MxZYaMdyBfgwp4d5rB9T1VQH5pJv5LtFJ");
pub const WSOL_MINT: Pubkey = pubkey!("So11111111111111111111111111111111111111112");
/// Pyth's SOL/USD feed id.
pub const SOL_USD_FEED: [u8; 32] = [
    0xef, 0x0d, 0x8b, 0x6f, 0xda, 0x2c, 0xeb, 0xa4, 0x1d, 0xa1, 0x5d, 0x40, 0x95, 0xd1, 0xda, 0x39,
    0x2a, 0x0d, 0x2f, 0x8e, 0xd0, 0xc6, 0xc7, 0xbc, 0x0f, 0x4c, 0xfa, 0xc8, 0xc2, 0x80, 0xb5, 0x6d,
];

const POOL_DISCRIMINATOR: [u8; 8] = [241, 154, 109, 4, 17, 177, 109, 188];
const SWAP_DISCRIMINATOR: [u8; 8] = [248, 198, 158, 145, 225, 117, 135, 200];
const PRICE_UPDATE_DISCRIMINATOR: [u8; 8] = [34, 241, 35, 99, 157, 126, 244, 205];
/// Byte offsets in a DAMM v2 `Pool` (checked against the mainnet IDL and a live pool).
const POOL_MINT_A: usize = 168;
const POOL_MINT_B: usize = 200;
const POOL_VAULT_A: usize = 232;
const POOL_VAULT_B: usize = 264;
const POOL_SQRT_PRICE: usize = 456;
const POOL_STATUS: usize = 481;

fn key_at(d: &[u8], at: usize) -> Pubkey {
    Pubkey::new_from_array(d[at..at + 32].try_into().unwrap())
}

pub struct DammPool {
    pub mint_a: Pubkey,
    pub mint_b: Pubkey,
    pub vault_a: Pubkey,
    pub vault_b: Pubkey,
    /// sqrt(token_b per token_a in base units), Q64.64.
    pub sqrt_price: u128,
}

pub fn read_damm_pool(pool: &AccountInfo) -> Result<DammPool> {
    require_keys_eq!(*pool.owner, DAMM_V2, ParanoidError::TokenPayInvalid);
    let d = pool.try_borrow_data()?;
    require!(d.len() > POOL_STATUS && d[..8] == POOL_DISCRIMINATOR, ParanoidError::TokenPayInvalid);
    require!(d[POOL_STATUS] == 0, ParanoidError::TokenPayInvalid); // 0 = enabled
    let sqrt_price = u128::from_le_bytes(d[POOL_SQRT_PRICE..POOL_SQRT_PRICE + 16].try_into().unwrap());
    require!(sqrt_price > 0, ParanoidError::TokenPayInvalid);
    Ok(DammPool {
        mint_a: key_at(&d, POOL_MINT_A),
        mint_b: key_at(&d, POOL_MINT_B),
        vault_a: key_at(&d, POOL_VAULT_A),
        vault_b: key_at(&d, POOL_VAULT_B),
        sqrt_price,
    })
}

/// SOL/USD from a fully verified Pyth price update no older than `max_age` seconds.
/// Returns (price, exponent): USD per SOL = price * 10^exponent.
pub fn read_sol_usd(update: &AccountInfo, now: i64, max_age: i64) -> Result<(u64, i32)> {
    require_keys_eq!(*update.owner, PYTH_RECEIVER, ParanoidError::PriceInvalid);
    let d = update.try_borrow_data()?;
    require!(d.len() >= 133 && d[..8] == PRICE_UPDATE_DISCRIMINATOR, ParanoidError::PriceInvalid);
    require!(d[40] == 1, ParanoidError::PriceInvalid); // VerificationLevel::Full
    let m = 41;
    require!(d[m..m + 32] == SOL_USD_FEED, ParanoidError::PriceInvalid);
    let price = i64::from_le_bytes(d[m + 32..m + 40].try_into().unwrap());
    let conf = u64::from_le_bytes(d[m + 40..m + 48].try_into().unwrap());
    let expo = i32::from_le_bytes(d[m + 48..m + 52].try_into().unwrap());
    let publish_time = i64::from_le_bytes(d[m + 52..m + 60].try_into().unwrap());
    require!(price > 0 && (-12..=0).contains(&expo), ParanoidError::PriceInvalid);
    // A wide confidence band means the market itself is unsure: refuse rather than misprice.
    require!(u128::from(conf) * 50 <= price as u128, ParanoidError::PriceInvalid);
    require!(publish_time.saturating_add(max_age) >= now, ParanoidError::PriceStale);
    Ok((price as u64, expo))
}

/// a * b / d with a 256-bit intermediate, as the quotient and whether a remainder was lost.
/// None on d == 0 or a quotient above u128.
fn mul_div_wide(a: u128, b: u128, d: u128) -> Option<(u128, bool)> {
    if d == 0 {
        return None;
    }
    // 128 x 128 -> 256 bits as (hi, lo).
    let (a1, a0) = (a >> 64, a & u64::MAX as u128);
    let (b1, b0) = (b >> 64, b & u64::MAX as u128);
    let p00 = a0 * b0;
    let p01 = a0 * b1;
    let p10 = a1 * b0;
    let p11 = a1 * b1;
    let mid = (p00 >> 64) + (p01 & u64::MAX as u128) + (p10 & u64::MAX as u128);
    let lo = (p00 & u64::MAX as u128) | (mid << 64);
    let hi = p11 + (p01 >> 64) + (p10 >> 64) + (mid >> 64);
    if hi == 0 {
        return Some((lo / d, lo % d != 0));
    }
    if hi >= d {
        return None; // quotient does not fit in 128 bits
    }
    // Long division of (hi, lo) by d, one bit at a time; hi < d keeps the remainder in range.
    let (mut r, mut q) = (hi, 0u128);
    for i in (0..128).rev() {
        let carry = r >> 127;
        r = (r << 1) | ((lo >> i) & 1);
        q <<= 1;
        if carry == 1 || r >= d {
            r = r.wrapping_sub(d);
            q |= 1;
        }
    }
    Some((q, r != 0))
}

/// ceil(a * b / d). None on d == 0 or a result above u128.
pub fn mul_div_ceil(a: u128, b: u128, d: u128) -> Option<u128> {
    let (q, lost) = mul_div_wide(a, b, d)?;
    if lost { q.checked_add(1) } else { Some(q) }
}

/// floor(a * b / d). None on d == 0 or a result above u128.
pub fn mul_div_floor(a: u128, b: u128, d: u128) -> Option<u128> {
    Some(mul_div_wide(a, b, d)?.0)
}

/// PARANOID base units that `lamports` of wSOL buys at the pool's spot price, rounded down.
/// The burn swap's minimum-out is a slice of this.
pub fn tokens_out_floor(lamports: u64, sqrt_price: u128, paranoid_is_a: bool) -> Option<u64> {
    const Q64: u128 = 1 << 64;
    let n = u128::from(lamports);
    let t = if paranoid_is_a {
        mul_div_floor(mul_div_floor(n, Q64, sqrt_price)?, Q64, sqrt_price)?
    } else {
        mul_div_floor(mul_div_floor(n, sqrt_price, Q64)?, sqrt_price, Q64)?
    };
    u64::try_from(t).ok()
}

/// One DAMM v2 swap: `amount_in` from `input_account` for at least `minimum_out` into
/// `output_account`. The referral slot carries the program id, DAMM's spelling of "none".
#[allow(clippy::too_many_arguments)]
pub fn swap<'info>(
    program: &AccountInfo<'info>,
    pool_authority: &AccountInfo<'info>,
    pool: &AccountInfo<'info>,
    input_account: &AccountInfo<'info>,
    output_account: &AccountInfo<'info>,
    vault_a: &AccountInfo<'info>,
    vault_b: &AccountInfo<'info>,
    mint_a: &AccountInfo<'info>,
    mint_b: &AccountInfo<'info>,
    payer: &AccountInfo<'info>,
    token_a_program: &AccountInfo<'info>,
    token_b_program: &AccountInfo<'info>,
    event_authority: &AccountInfo<'info>,
    amount_in: u64,
    minimum_out: u64,
    signer_seeds: &[&[&[u8]]],
) -> Result<()> {
    let mut data = Vec::with_capacity(24);
    data.extend_from_slice(&SWAP_DISCRIMINATOR);
    data.extend_from_slice(&amount_in.to_le_bytes());
    data.extend_from_slice(&minimum_out.to_le_bytes());
    let accounts = vec![
        AccountMeta::new_readonly(*pool_authority.key, false),
        AccountMeta::new(*pool.key, false),
        AccountMeta::new(*input_account.key, false),
        AccountMeta::new(*output_account.key, false),
        AccountMeta::new(*vault_a.key, false),
        AccountMeta::new(*vault_b.key, false),
        AccountMeta::new_readonly(*mint_a.key, false),
        AccountMeta::new_readonly(*mint_b.key, false),
        AccountMeta::new_readonly(*payer.key, true),
        AccountMeta::new_readonly(*token_a_program.key, false),
        AccountMeta::new_readonly(*token_b_program.key, false),
        AccountMeta::new_readonly(*program.key, false),
        AccountMeta::new_readonly(*event_authority.key, false),
        AccountMeta::new_readonly(*program.key, false),
    ];
    let infos: Vec<AccountInfo<'info>> = vec![
        pool_authority.clone(), pool.clone(), input_account.clone(), output_account.clone(),
        vault_a.clone(), vault_b.clone(), mint_a.clone(), mint_b.clone(), payer.clone(),
        token_a_program.clone(), token_b_program.clone(), event_authority.clone(), program.clone(),
    ];
    invoke_signed(&Instruction { program_id: *program.key, accounts, data }, &infos, signer_seeds)?;
    Ok(())
}

/// PARANOID base units to charge for `usdc` base units (6 decimals) of questions, plus `premium_bps`
/// (negative for a discount). The pool gives sqrt(token_b / token_a) in base units; SOL has 9
/// decimals, USDC 6, and the PARANOID decimals cancel because the pool price is already in base
/// units. Rounds up.
pub fn token_cost(usdc: u64, premium_bps: i16, sqrt_price: u128, paranoid_is_a: bool, sol_usd: u64, expo: i32) -> Option<u64> {
    const Q64: u128 = 1 << 64;
    // usdc * (1 + premium) * 1e3 (lamports per USDC unit at $1/SOL) * 10^-expo
    let factor = u128::try_from(i64::try_from(BPS).ok()? + i64::from(premium_bps)).ok()?;
    let n = u128::from(usdc)
        .checked_mul(factor)?
        .checked_mul(1_000)?
        .checked_mul(10u128.checked_pow(u32::try_from(-expo).ok()?)?)?;
    // lamports per PARANOID base unit = (sqrt/2^64)^2 when PARANOID is token a, else its inverse.
    let t = if paranoid_is_a {
        mul_div_ceil(mul_div_ceil(n, Q64, sqrt_price)?, Q64, sqrt_price)?
    } else {
        mul_div_ceil(mul_div_ceil(n, sqrt_price, Q64)?, sqrt_price, Q64)?
    };
    let d = u128::from(sol_usd).checked_mul(u128::from(BPS))?;
    let tokens = t.checked_add(d - 1)? / d;
    u64::try_from(tokens).ok().filter(|&x| x > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn mul_div_examples() {
        assert_eq!(mul_div_ceil(10, 10, 3), Some(34));
        assert_eq!(mul_div_ceil(u128::MAX, 2, 2), Some(u128::MAX));
        assert_eq!(mul_div_ceil(1 << 100, 1 << 100, 1 << 90), Some(1 << 110));
        assert_eq!(mul_div_ceil(u128::MAX, u128::MAX, 1), None);
        assert_eq!(mul_div_ceil(1, 1, 0), None);
    }

    #[test]
    fn live_pool_example() {
        // Mainnet 2026-10-25: sqrt_price of 5Diq3h... and SOL at $101.042 (expo -8).
        // PARANOID ~ $2.77e-5, so 5 USDC + 10% ~ 198,000 PARANOID.
        let sol_per_paranoid: f64 = 2.7727e-5 / 101.042;
        let sqrt = (sol_per_paranoid.sqrt() * 2f64.powi(64)) as u128;
        let t = token_cost(5_000_000, 1_000, sqrt, true, 10_104_200_000, -8).unwrap();
        let whole = t as f64 / 1e9;
        assert!((whole - 5.5 / 2.7727e-5).abs() / whole < 1e-6, "{whole}");
        // Same pool with the sides swapped prices the same.
        let inv = ((1.0 / sol_per_paranoid).sqrt() * 2f64.powi(64)) as u128;
        let t2 = token_cost(5_000_000, 1_000, inv, false, 10_104_200_000, -8).unwrap();
        assert!((t2 as f64 - t as f64).abs() / (t as f64) < 1e-6);
    }

    proptest! {
        #[test]
        fn mul_div_matches_u128(a in any::<u64>(), b in any::<u64>(), d in 1u128..u128::MAX) {
            let exact = (a as u128) * (b as u128);
            let want = exact / d + u128::from(exact % d != 0);
            prop_assert_eq!(mul_div_ceil(a as u128, b as u128, d), Some(want));
        }

        #[test]
        fn mul_div_inverts(a in any::<u128>(), b in 1u128..u128::MAX) {
            prop_assert_eq!(mul_div_ceil(a, b, b), Some(a));
        }

        /// Charging never undercuts the asked premium, and a higher premium (or a smaller discount)
        /// never charges less.
        #[test]
        fn cost_monotone(usdc in 1u64..200_000_000, prem in -5_000i16..5_000, sqrt in (1u128 << 40)..(1u128 << 90)) {
            if let (Some(lo), Some(hi)) = (token_cost(usdc, prem, sqrt, true, 10_000_000_000, -8), token_cost(usdc, prem + 1, sqrt, true, 10_000_000_000, -8)) {
                prop_assert!(hi >= lo);
            }
        }
    }
}
