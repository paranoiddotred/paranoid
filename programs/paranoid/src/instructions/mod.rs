pub mod burn;
pub mod deposit;
pub mod initialize;
pub mod stake;
pub mod order;
pub mod slash;
pub mod sweep;
pub mod top_up;
pub mod token_pay;

pub use burn::*;
pub use deposit::*;
pub use initialize::*;
pub use stake::*;
pub use order::*;
pub use slash::*;
pub use sweep::*;
pub use top_up::*;
pub use token_pay::*;

use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Transfer};

use crate::constants::CONFIG_SEED;
use crate::error::ParanoidError;
use crate::math::{price, split};
use crate::state::Config;

/// Share of every purchase sent to the burn vault: all of it since 2026-10-27 (half from 2026-10-26).
/// The buyback hands half of what it buys to stakers and burns the rest; the gateway's model costs are
/// paid from the token's trading fees. The launch constant `burn_bps` froze at 2,000 when the first
/// member joined, so the share lives here instead of in the account.
pub(crate) const PURCHASE_BURN_BPS: u16 = 10_000;

/// Moves `amount` out of a token account owned by the config PDA.
pub(crate) fn pay_from<'info>(
    token_program: &Program<'info, Token>,
    from: &Account<'info, TokenAccount>,
    to: &AccountInfo<'info>,
    config: &Account<'info, Config>,
    amount: u64,
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    let seeds: &[&[u8]] = &[CONFIG_SEED, &[config.bump]];
    token::transfer(
        CpiContext::new_with_signer(
            token_program.to_account_info(),
            Transfer { from: from.to_account_info(), to: to.clone(), authority: config.to_account_info() },
            &[seeds],
        ),
        amount,
    )
}

/// Takes payment for `questions` tickets straight from the buyer: the burn share to the burn vault,
/// the rest to the treasury vault. Nothing is held for the member, so there is nothing to refund.
/// Returns the amount paid.
pub(crate) fn take_payment<'info>(
    config: &mut Account<'info, Config>,
    questions: u64,
    buyer: &Signer<'info>,
    source: &Account<'info, TokenAccount>,
    treasury_vault: &Account<'info, TokenAccount>,
    burn_vault: &Account<'info, TokenAccount>,
    token_program: &Program<'info, Token>,
) -> Result<u64> {
    let amount = price(config.rate, questions).ok_or(ParanoidError::MathOverflow)?;
    let (to_burn, to_treasury) = split(amount, PURCHASE_BURN_BPS);
    for (to, part) in [(treasury_vault, to_treasury), (burn_vault, to_burn)] {
        if part == 0 {
            continue;
        }
        token::transfer(
            CpiContext::new(
                token_program.to_account_info(),
                Transfer { from: source.to_account_info(), to: to.to_account_info(), authority: buyer.to_account_info() },
            ),
            part,
        )?;
    }
    config.deposited = config.deposited.checked_add(amount).ok_or(ParanoidError::MathOverflow)?;
    config.paid_treasury = config.paid_treasury.checked_add(to_treasury).ok_or(ParanoidError::MathOverflow)?;
    config.paid_burn = config.paid_burn.checked_add(to_burn).ok_or(ParanoidError::MathOverflow)?;
    Ok(amount)
}

/// A purchase must buy at least `min_questions` and keep the membership at or under `limit`.
pub(crate) fn check_questions(config: &Config, bought: u64, total: u64) -> Result<()> {
    require!(bought >= u64::from(config.min_questions) && total <= u64::from(config.limit), ParanoidError::InvalidQuestions);
    Ok(())
}
