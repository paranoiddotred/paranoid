# PARANOID trusted setup (Groth16, BN254), 2026-10-18 UTC

Replaces the local single-party setup. Circuit `circuits/request.circom` (Request(20, 4)), unchanged.

## Phase 1: public
`powersOfTau28_hez_final_15.ptau` (Hermez perpetual powers of tau, 54 contributions + beacon), from
https://circom.info/powersOfTau28_hez_final_15.ptau (the address in the current snarkjs README; the old
Google and S3 addresses return 403). blake2b, matches the README:
982372c867d229c236091f767e703253249a9b432c1710b4f326306bfa2428a17b06240359606cfe4d580b10a5a1f63fbed499527069c18ae17060472969ae6e

## Phase 2
1. `snarkjs groth16 setup build/request.r1cs <phase 1> request_0000.zkey`
2. Contribution 1, 64 bytes of /dev/urandom passed straight to `snarkjs zkey contribute`, never written down.
   Contribution hash: d8597c6d 42000388 219bed8a 3ea21ba0 1b2ac85f c3bf0519 769f4eff 38665ff1 9e325726 a6d5613c cfe84110 024ea207 dadbc08f aef893a9 ed0c67f4 18176434
3. Contribution 2, the same way with separate randomness.
   Contribution hash: 617fa871 e479140d ac2dcac1 4ebecc1f 993184f0 f37f7cbf 455dfaf6 432f330d 90565a5f 0dd91ae0 b33f2abf a5f70c4d 46040d33 803aee37 6f7f7336 36bf6ba7
   sha256 of request_0002.zkey: b9b83fee6d1907654dffe0889dff4d7e7bc9013795818465fdf99c79d7f1686b
4. Beacon committed at 2026-10-18T21:30:53Z, when round 6564267 was the latest: drand mainnet (https://api.drand.sh) round 6564273.
   Randomness: 182365c0e863aff1096b07fae6495b3c7670857258daf39a3448d1ed6c483baf
   `snarkjs zkey beacon request_0002.zkey request_final.zkey <randomness> 10 -n="drand mainnet round 6564273"`
5. `snarkjs zkey verify build/request.r1cs <phase 1> build/request.zkey` prints `ZKey Ok!` (build/ceremony/verify.txt).

## Result (sha256)
- build/request_js/request.wasm  f44c7e806a7fdf45d4fca05b02604594bd19f0d47ef626272c7b87228ec7b75d
- build/request.zkey             722239f79e97f872523d3f6dc7c04706bb7efc74959d9b40483b2a6f7f14eec6
- build/verification_key.json    a9877f51a1b0f6316f2c7ce5716359e9bb1d19564c0488e0f4a8019c87f859ec

The app and the gateway serve these same files (same bytes). bench/src/vk.rs regenerated and the bench rebuilt.
Engine suite after the switch: 30 passed, 0 failed. Product sdk-selftest: prove, verify, slash recovery and tamper checks all pass.

## What it means
Soundness rests on at least one phase 2 contributor having destroyed their randomness. Both contributions
were made on the operator's machine, so today that means trusting that machine. A broken setup can forge
proofs (free requests); it can never reveal who asked. The program does not embed the key: more people can
add contributions at any time (`snarkjs zkey contribute` on the current key, then a new beacon and
a copy to the app) and the gateway switches keys with no on-chain change.
