//! Money rules, kept free of accounts so they can be property-tested on the host.

use crate::constants::BPS;

/// Price of `questions` tickets at `rate` each.
pub fn price(rate: u64, questions: u64) -> Option<u64> {
    rate.checked_mul(questions)
}

/// Splits `amount` into (burn share, rest). The burn share rounds down, so the rest never shrinks
/// below its exact share and the two always add back to `amount`.
pub fn split(amount: u64, burn_bps: u16) -> (u64, u64) {
    let burn = (u128::from(amount) * u128::from(burn_bps) / u128::from(BPS)) as u64;
    (burn, amount - burn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn split_examples() {
        assert_eq!(split(1_000_000, 1_000), (100_000, 900_000));
        assert_eq!(split(1, 9_999), (0, 1));
        assert_eq!(split(u64::MAX, 10_000), (u64::MAX, 0));
    }

    proptest! {
        #[test]
        fn split_conserves(amount in any::<u64>(), bps in 0u16..=10_000) {
            let (burn, rest) = split(amount, bps);
            prop_assert_eq!(burn as u128 + rest as u128, amount as u128);
            prop_assert!(burn as u128 * 10_000 <= amount as u128 * bps as u128);
        }

        /// A purchase split between treasury and burn always adds back to its price.
        #[test]
        fn purchase_split_conserves(rate in 1u64..1_000_000_000, questions in 1u64..65_536, bps in 0u16..=10_000) {
            let amount = price(rate, questions).unwrap();
            let (burn, rest) = split(amount, bps);
            prop_assert_eq!(burn + rest, amount);
        }
    }
}
