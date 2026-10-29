# Security

Scope: the Anchor program in `programs/paranoid`, the request circuit in `circuits/request.circom`, and the
reference library in `sdk/` that clients and gateways run. It has not been audited by a third party yet.

## Reporting a problem

Please report it privately through **Report a vulnerability** on this repository's Security tab, not in a
public issue. Include the instruction or circuit constraint involved and, if you can, a failing test.

## Trust model

| Party | Trusted for | Not trusted for |
| --- | --- | --- |
| Upgrade authority | Program upgrades, and the $PARANOID payment premium within 50% either way | Moving payments, stakes or order escrows: none of its instructions can |
| Gateway | Serving the questions it was paid for | Knowing who asked: it sees proofs and questions, never which member sent them. No instruction gives it any power over the program's accounts |
| Groth16 setup | Soundness: a broken setup could forge proofs, which means free questions | Privacy: Groth16 is zero-knowledge for any setup, so even a broken one cannot reveal who asked |
| Pyth SOL/USD | Pricing $PARANOID payments and `convert_burn` | Stale or unverified prices: every read checks the account owner, the feed id, full verification and an age of at most 60 seconds, and refuses a price whose confidence band is wider than 2% of it |
| Meteora DAMM v2 pool | The venue for the buyback, and the token's price for token payments | Being pushed first: see "Why moving the pool does not pay" |
| USDC (Circle) | Not freezing the program's token accounts | |

## What each check defends against

**Payments**
- `deposit` and `top_up` take `rate × questions` straight from a token account the buyer signs for and send
  it to the burn vault. Nothing is held per member, so there is nothing to refund and nothing to drain.
- A purchase must buy at least `min_questions` and keep the membership at or under `limit`.
- The member account is derived from the commitment, so one commitment holds one membership, and a slashed
  commitment can never join again. A zero commitment, or one that is not a BN254 field element, is refused.
- `sweep_treasury` can only pay the treasury account fixed at launch.
- A gift card order holds at most 1,000 USDC. Only the key it names or its buyer can settle it, and the buyer
  can only send the money to a USDC account the buyer owns.
- All arithmetic is checked. `split` rounds the burn share down and both parts always add back up.

**Member tree and roots**
- Changing a leaf first recomputes the current root from the supplied siblings, so a wrong index, leaf or
  path fails with `InvalidPath`. A property test checks that every edit equals a full recomputation.
- After `top_up`, roots holding the old leaf stay accepted. The old leaf only proves fewer tickets of the
  same secret, and a ticket has the same nullifier under any root, so it still cannot be spent twice.
- `slash` caps every older root from the current day, so a secret that became public cannot be used
  against any root a gateway still accepts.

**Tickets and slashing**
- The circuit proves that the spent tickets lie below the count bound into the member's own leaf, binds the
  proof to the message hash, the day and the root, and pins unused outputs to zero. Ticket numbers never
  reset, so each ticket can be spent once in a membership's life, and the only way to spend more is to
  reuse one, which publishes a second point of its line.
- `slash` accepts a secret only if its Poseidon hash equals the member's commitment. The reporter gets
  nothing, so copying someone else's slash transaction gains nothing.

**Buyback and burn**
- `convert_burn` sells at most 25 USDC of the burn vault per call at the oracle price, and the caller's pay
  is fixed at 0.3% of it.
- `crank_burn` checks the pool, the token and the vaults against the configuration, spends at most 0.2% of
  the pool's SOL side and waits 150 slots after the previous buy. Its 97% floor on the pool's own quote
  covers rounding and drift; the next section covers a pool pushed on purpose.
- Half of every buy is shared among stakes that are not unstaking through a per-token accumulator (scale
  10^12), and the rest is burned. With nothing staked, all of it is burned.

**Staking**
- One stake per wallet. Only its owner can add to it, claim from it, ask to unstake or withdraw, and every
  payout goes to a $PARANOID account the owner holds. Withdrawal opens three days after asking to unstake,
  and a stake stops earning the moment it asks, so arriving just before a buyback means three locked days.

**Launch powers**
- `initialize`, `set_burn_target`, `reconfigure`, `set_token_payment` and `set_token_premium` check the
  signer against the program's upgrade authority. `initialize`, `set_burn_target` and `set_token_payment`
  create their accounts with `init`, so each runs once. `reconfigure` works only while nobody has ever
  bought, so the launch constants are fixed for good once the first member joins.

## Why moving the pool does not pay

Two instructions read the Meteora pool's current price, so the fair question is what someone gains by pushing
that price in the same transaction. The pool charges 2% on each leg of a trade, and the program keeps its
own use of the pool small, so the push costs more than it can win.

- **The buyback.** Pushing the price before `crank_burn` and selling after costs the mover about 4% of the
  amount pushed in fees. The crank spends at most 0.2% of the pool's SOL side, so the most it can be made to
  overpay is about 0.4% of that amount. The push loses about ten times what it gains.
- **Token payments.** Pushing the price up makes a token payment burn fewer tokens, which pays off only when
  one payment is worth more than 2% of the pool's SOL side. A membership holds at most 65,535 questions,
  about 131 USDC at list price, and on 2026-10-30 the pool held about 170 SOL, so a push would need a single
  payment above about 3.4 SOL. Whatever is gained is questions, never money out of the program, and the
  buyer's own `max_tokens` guards the payer in the other direction.

## Design decisions

- **Privacy needs no trusted party.** The trusted setup only governs soundness. It is recorded in
  CEREMONY.md, and more contributions can be added at any time without touching the program, because the
  verifying key lives with the gateway rather than on-chain.
- **What is public, and what never is.** The buying wallet, its commitment and how many questions it bought
  are public. Which member asked which question is never on-chain or in a proof.
- **A request hides among every valid membership.** A membership stays valid for 365 days after its last
  purchase, so a request hides among every member who bought or topped up in the past year and was not
  slashed.
- **A double use is caught before it is served.** The gateway keeps the nullifiers it has seen and refuses a
  ticket's second use before forwarding it. If two uses ever reach it, the secret comes out and anyone can
  slash the member.

## Tooling

- `cargo clippy --all-targets -- -D warnings`: clean.
- `bash scripts/test.sh`: 45 passed, 0 failed, none skipped (12 Rust unit and property tests, 33 Node tests on
  the circuit and on a local validator, including the 120-step solvency fuzz).
- `scripts/build.sh` reproduces the deployed program byte for byte (README, "What you can check yourself").
