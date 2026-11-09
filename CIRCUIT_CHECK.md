# Circuit check

The most common way a zero-knowledge system breaks is an under-constrained circuit: a signal the constraints
do not pin down, so a prover can pick its value freely and a fake proof still verifies. For PARANOID that would
mean spending tickets without paying, or reusing a ticket without the slashing line that exposes the secret.

We ran two independent tools on `circuits/request.circom`, Request(20, 4) (sha256 of the source
`b05120d256645e58...`), on 2026-11-09.

## Picus (Veridise): every output is uniquely determined

[Picus](https://github.com/Veridise/Picus) checks that, for any given inputs, the constraints allow exactly one
value for every output. If two different outputs satisfied the constraints for the same inputs, a prover could
choose between them, and Picus would print both as a counterexample.

- Picus at commit 138b151, cvc5 1.4.2 with finite field support (the GPL build, CoCoA enabled), circom 2.2.3.
- The circuit compiled without optimisation, so every constraint the source writes is present:
  `circom circuits/request.circom --r1cs --O0` (24,199 wires, r1cs sha256 `c36e48460253d6eb...`).
- `run-picus --solver cvc5 --timeout 20000 request.r1cs` printed **"The circuit is properly constrained"**
  (exit code 8) in about 25 seconds. All outputs, the four nullifiers and the four line points `ys`, are fixed
  by the public inputs (root, epoch, x, limit, weight) and the private inputs.

Controls, so the result is not the tool saying yes to everything:

- circomlib's `Decoder` from Picus's own benchmarks, a known under-constrained template: "The circuit is
  underconstrained" (exit code 9).
- Our circuit with `line[j] <== ...` changed to `line[j] <-- ...` (an assignment with no constraint): Picus does
  not certify it ("Cannot determine whether the circuit is properly constrained", exit code 0).
- Our circuit with `nullifiers[j] <== ...` changed to `<--`: not certified either (exit code 0).

The production key is built from the `--O2` compilation (`scripts/setup-circuit.sh`, 7,467 constraints), where
circom's optimiser has merged linear constraints. Picus ran on that file for an hour without a verdict either
way, so the uniqueness proof above is for the circuit as written. The `--O2` file is circom's own
simplification of the same constraints.

What uniqueness does not cover: it says nothing is left free, not that the rules are the right rules. Whether
the circuit checks membership, the epoch and the ticket window as intended is covered by `tests/circuit.test.ts`:
no proof exists past the tickets bought, past the last valid day, for weight 0 or above 4, above the protocol
cap, or for a leaf that is not in the tree; a ticket used twice gives the secret back.

## circomspect (Trail of Bits): one warning, not a bug

[circomspect](https://github.com/trailofbits/circomspect) 0.9.0, `circomspect circuits/request.circom -L node_modules`:

```
warning: Inputs to `LessThan` need to be constrained to ensure that they are non-negative
118 │         active[j].in[0] <== j;
    │                             ^ `j` needs to be constrained to ensure that it is <= p/2.
```

`j` is the loop counter, fixed when the circuit is compiled to 0, 1, 2 and 3. It is a constant in the
constraints, not a signal a prover controls, so it cannot be negative or large. The other input of that
comparison, `weight`, is range-checked to 8 bits by `Num2Bits(8)` a few lines earlier. No other issue was
reported.

## Reproduce

```sh
npm ci                                            # circomlib
circom circuits/request.circom --r1cs --O0 -o /tmp/picus
git clone https://github.com/Veridise/Picus && cd Picus && raco pkg install --auto
PATH=/path/to/cvc5-gpl/bin:$PATH ./run-picus --solver cvc5 --timeout 20000 /tmp/picus/request.r1cs
circomspect circuits/request.circom -L node_modules
```
