# PARANOID trusted setup (Groth16, BN254), v2, 2026-10-23 UTC

For the v2 circuit (question packs): `circuits/request.circom`, Request(20, 4), 7,467 constraints,
5 public inputs. The v1 ceremony of 2026-10-18 is kept in `build/v1/CEREMONY-v1.md` with its logs.

## Phase 1: public
`powersOfTau28_hez_final_15.ptau` (Hermez perpetual powers of tau, 54 contributions + beacon), from
https://circom.info/powersOfTau28_hez_final_15.ptau. blake2b, matches the snarkjs README:
982372c867d229c236091f767e703253249a9b432c1710b4f326306bfa2428a17b06240359606cfe4d580b10a5a1f63fbed499527069c18ae17060472969ae6e

## Phase 2
1. `snarkjs groth16 setup build/request.r1cs <phase 1> request_0000.zkey`
2. Contribution 1, 64 bytes of /dev/urandom passed straight to `snarkjs zkey contribute`, never written down.
   Contribution hash: 120ee06c 7f2f1903 8626afe3 bf84c0e9 6cf5ee7f edf4b188 0f03648b 741b451a fcf0f565 6f7d6b9a b007c214 0e6f0478 d90784ff 27b7d26e 8563fbeb db56d933
3. Contribution 2, the same way with separate randomness.
   Contribution hash: afb1c203 090cba49 119a80d6 c1f2e334 2a397663 de7abb08 5251a5d7 857a17cd b5adb5bf a8c3f186 964d9dd0 36517b27 da7c7d89 9c7b4d3b 07f2925c f1cab7dd
   sha256 of request_0002.zkey: 63d167874518418b543e73018f7b4cba72ab1a49bd7bb2a08018b5e19967edb2
4. Beacon committed at 2026-10-23T16:03:20Z, when round 6578012 was the latest: drand mainnet (https://api.drand.sh) round 6578018.
   Randomness: 823eab27e163fafa0e85ec11a2889c506c60721f8412be69391c29eff703dc3e (build/ceremony/drand-6578018.json)
   `snarkjs zkey beacon request_0002.zkey request.zkey <randomness> 10 -n="drand mainnet round 6578018"`
   Beacon contribution hash: 82fa9266 33080d8c 275f432f 119fada1 678c19b4 61485f1c f58f5df8 11e48254 bfefbfa5 097b1fbf 39d89987 54d56ba6 d5353492 a869e366 e8bdbf0d c7fa3cf9
5. `snarkjs zkey verify build/request.r1cs <phase 1> build/request.zkey` prints `ZKey Ok!` (build/ceremony/verify.txt).

## Result (sha256)
- build/request_js/request.wasm  63ddec9cd39bb2eb06c8c6fa51eb72e4333863fb8757c68770ab4ab1abe06478
- build/request.zkey             d909d787fd228867708c361fef699676bc49a5c0adfe140a6d50a539779e69cd
- build/verification_key.json    ee128f7ee298574f043e7710e5c798e1c0bb6c5f35fe05b9564f05bd4daf483e

The app and the gateway serve these same files (same bytes). bench/src/vk.rs regenerated and the bench rebuilt.
Engine suite after the switch: 29 passed, 0 failed.

## What it means
Soundness rests on at least one phase 2 contributor having destroyed their randomness. Both contributions
were made on the operator's machine, so today that means trusting that machine. A broken setup can forge
proofs (free questions); it can never reveal who asked. The program does not embed the key: more people can
add contributions at any time (`snarkjs zkey contribute` on the current key, then a new beacon and
a copy to the app) and the gateway switches keys with no on-chain change.
