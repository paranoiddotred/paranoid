# PARANOID

PARANOID lets people ask AI models questions without the service learning who asked. A member buys a
pack of questions once, in USDC or in $PARANOID, and then sends every question with a fresh
zero-knowledge proof that it was paid for. The gateway checks the proof and forwards the question, but the
proof does not say which member sent it, and two questions from the same member cannot be tied together.

This repository is the on-chain part: the Solana program that sells questions and keeps the member list,
the circuit every question is proven with, and their tests. The app and the gateway run at
[paranoid.red](https://paranoid.red). The connector for Claude Code, Cursor and other MCP clients is
[paranoiddotred/paranoid-mcp](https://github.com/paranoiddotred/paranoid-mcp).

## What you can check yourself

- **This source is the deployed program, byte for byte.** Built with the toolchain pinned in
  [`scripts/build.sh`](scripts/build.sh), it produces exactly the bytes at
  `9JJqzEoDGkTE8CMLATRw7CqFhcyWVegfhFuEeGfBfVGm` on mainnet. The commands are below.
- **Privacy rests on math, not on a key.** Groth16 is zero-knowledge for any setup, so even a broken trusted
  setup could at worst forge free questions. It could never reveal who asked.
- **45 tests, none skipped.** Rust unit and property tests check that every Merkle edit equals a full
  recomputation and that the price math matches a 128-bit reference. On a local validator, a 120-step random
  fuzz of purchases, top-ups, slashes and sweeps checks after every step that each vault and each wallet adds
  up to the base unit and that the SDK's tree equals the chain's root. Further suites cover the circuit,
  the program lifecycle, token payments, orders and staking.
- **A request proof verifies on Solana in 174,478 compute units**, measured by the bench program in `bench/`
  with the `alt_bn128` syscalls (one 882-byte transaction). A tampered proof is rejected.
- **The trusted setup is on record.** The Hermez phase 1, two phase 2 contributions and a drand beacon
  (round 6578018), with transcripts and hashes in [`CEREMONY.md`](CEREMONY.md) and `build/ceremony/`. The
  proving key and witness generator here are the same files the app downloads.
- **Clippy is clean** with `cargo clippy --all-targets -- -D warnings`.

```bash
npm ci
bash scripts/build.sh
solana program dump 9JJqzEoDGkTE8CMLATRw7CqFhcyWVegfhFuEeGfBfVGm onchain.so -u mainnet-beta
head -c "$(wc -c < target/deploy/paranoid.so | tr -d ' ')" onchain.so | cmp - target/deploy/paranoid.so && echo same
bash scripts/test.sh
```

The build uses `cargo-build-sbf` from Agave 2.1.0 (`agave-install init 2.1.0`), platform-tools v1.52 and
Anchor 0.31.1 through avm, and the byte match was checked on macOS. The Agave version matters, because
`cargo-build-sbf` from 2.3 on compiles for a newer target and gives different bytes from the same source.
The deployed account is longer than the build only by zero bytes of spare room. The tests need Rust, the
Solana CLI and Node.js 22.18 or newer, which runs the TypeScript tests directly.

## Where the design comes from

PARANOID did not invent this design. It is zkAPI, released on 2026-10-01 by the Ethereum Foundation and
the Open Anonymity Project, from a design Vitalik Buterin and Davide Crapis proposed in February 2026:
deposit once, make paid calls that cannot be linked, and catch anyone who spends the same ticket twice with
rate-limit nullifiers (RLN). Their reference code is [vbuterin/zkapi](https://github.com/vbuterin/zkapi).
This repository builds that idea on Solana, which checks Poseidon hashes and BN254 pairings natively.

## How a question is paid for

1. The member's device makes a secret and only ever publishes its Poseidon hash, the commitment.
2. `deposit` takes `rate × questions` USDC from the buyer's wallet and adds the leaf
   `Poseidon(commitment, end_day, questions)` to a depth-20 Poseidon Merkle tree the program keeps. A
   purchase buys at least `min_questions`, a membership holds at most `limit`, and it stays valid for
   `max_epochs` days. `top_up` buys more for the same membership and renews it.
3. For each question the device proves with [`circuits/request.circom`](circuits/request.circom) that some
   leaf in a recent tree is its own and still valid today, and that the tickets it spends are numbered below
   the count that leaf bought. The proof is bound to the hash of the message and reveals one nullifier per
   ticket and nothing about which leaf was used.
4. The gateway checks the proof against a root the program stored (the last 64 are kept), refuses any
   nullifier it has already seen, and forwards the question.
5. Each ticket is a line `y = secret + a1 * x`. Spending a ticket once shows one point of its line.
   Spending it on two different messages shows two points, which gives away the secret, and anyone holding
   the secret can call `slash`. That removes the leaf from the tree, voids every ticket the member had left,
   and stops the gateway from accepting older roots that still contain it. The reporter earns nothing, so
   there is no reason to front-run a slash.

The buyer's wallet is public and tied to its commitment and to how many questions it bought. Questions are
never tied to either.

## On mainnet

Program `9JJqzEoDGkTE8CMLATRw7CqFhcyWVegfhFuEeGfBfVGm`. The launch constants live in its `Config`
account (PDA seed `config`): a day lasts 86,400 seconds, a question costs 0.002 USDC, a purchase buys at
least 250 questions, a membership holds at most 65,535 and stays valid for 365 days after its last
purchase. They are fixed for good, because `reconfigure` only works while nobody has ever bought.

The $PARANOID mint is `22rWiyDiY9GgxSrFiWC2vX1GxNxjkvrRPmZJDzv9pq6X`
([Solscan](https://solscan.io/token/22rWiyDiY9GgxSrFiWC2vX1GxNxjkvrRPmZJDzv9pq6X)), the same address as `PARANOID_MINT` in
[`constants.rs`](programs/paranoid/src/constants.rs). It has no mint authority, so the supply only goes down.

## Where the money goes

| Instruction | Who can call it | What it does |
| --- | --- | --- |
| `deposit`, `top_up` | the buyer | Every USDC paid goes to the program's burn vault (all of it since 2026-10-27). |
| `deposit_token`, `top_up_token` | the buyer | Prices the questions in $PARANOID through its Meteora pool and Pyth's SOL/USD, with a premium or discount of at most 50% (a 10% discount today), and burns every token paid in the same instruction. The buyer sets the most tokens it will pay. |
| `convert_burn` | anyone | Sells up to 25 USDC of the burn vault per call for wrapped SOL at Pyth's SOL/USD price. The caller keeps 0.3% of the price as pay. |
| `crank_burn` | anyone, once every 150 slots | Swaps the gathered SOL for $PARANOID in its Meteora DAMM v2 pool, spending at most 0.2% of the pool's SOL side. Half of what it buys goes to stakers, the rest is burned, and all of it is burned while nobody stakes. |
| `sweep_treasury` | anyone | Moves the treasury vault to the treasury account fixed at launch. |
| `open_order`, `settle_order` | the buyer, and the key the order names | Escrow for a gift card order, at most 1,000 USDC. Only the named key or the buyer can move it, and the buyer can only send it back to a USDC account of their own. |

## Staking

A wallet locks $PARANOID with `start_stake` and `add_stake` and shares every buyback while it stays
staked. `claim_stake_reward` sends what it has earned. `request_unstake` stops the earning and starts a
three-day wait, after which `withdraw_stake` returns the stake together with anything not yet claimed, so a
stake that arrives just before a buyback stays locked for three days after it. `return_legacy_stake` hands
back stakes from the earlier design, which sat on a membership and earned questions until 2026-10-27.

## Every privileged instruction

These check the signer against the program's upgrade authority, and there are no others.

| Instruction | What it can do |
| --- | --- |
| `initialize` | Create the configuration, once. |
| `reconfigure` | Replace the launch constants, only while nobody has ever bought. |
| `set_burn_target` | Name the token and pool the buyback uses, once. |
| `set_token_payment` | Name the token, pool and price feed for token payments, once. |
| `set_token_premium` | Move the token payment premium, within 50% either way. |

None of them moves payments, staked tokens or order escrows. Beyond these, the authority can upgrade the
program, which is how a change like this repository's next version would reach mainnet.

## Layout

| Path | What is there |
| --- | --- |
| `programs/paranoid` | The Anchor 0.31.1 program |
| `circuits/request.circom` | The request circuit, `Request(20, 4)` |
| `build/` | The proving key, verifying key, witness generator and ceremony transcript |
| `sdk/` | The reference client and gateway library: member tree, proofs, RLN, chain reads |
| `tests/` | The test suites run by `scripts/test.sh` |
| `bench/` | A small program that measures on-chain Groth16 verification |
| `fixtures/` | Mainnet accounts and programs the local validator loads, and throwaway test keys that hold nothing on mainnet |

## Security

Not audited by a third party yet. [SECURITY.md](SECURITY.md) explains what each check defends against, why
moving the pool does not pay, and how to report a problem privately.

## License

[MIT](LICENSE)
