#!/usr/bin/env bash
# Reproducible build: cargo-build-sbf from Agave 2.1.0 on platform-tools v1.52 (rustc 1.89), Anchor 0.31.1.
# Agave 2.1.0 builds for the sbf-solana-solana target the mainnet program was built for; cargo-build-sbf
# from 2.3 on builds for sbpf-solana-solana and gives different bytes from the same source.
# CARGO_BUILD_SBF points at another cargo-build-sbf.
#   target/deploy/paranoid.so           mainnet build (epochs of at least one hour)
#   target/deploy-localnet/paranoid.so  local validator build (epochs may last seconds)
#   target/idl/paranoid.json
#   target/bench/groth16_bench.so      on-chain verification bench (bench/, never deployed), for the tests
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
SBF="${CARGO_BUILD_SBF:-$HOME/.local/share/solana/install/releases/2.1.0/solana-release/bin/cargo-build-sbf}"
"$SBF" --tools-version v1.52 --manifest-path programs/paranoid/Cargo.toml --sbf-out-dir target/deploy
mkdir -p target/deploy-localnet && cp target/deploy/paranoid-keypair.json target/deploy-localnet/
"$SBF" --tools-version v1.52 --manifest-path programs/paranoid/Cargo.toml --sbf-out-dir target/deploy-localnet --features localnet
mkdir -p target/idl target/types
"$HOME/.avm/bin/anchor-0.31.1" idl build -p paranoid -o target/idl/paranoid.json -t target/types/paranoid.ts
"$SBF" --tools-version v1.52 --manifest-path bench/Cargo.toml --sbf-out-dir target/bench
echo "\"$(solana-keygen pubkey target/bench/groth16_bench-keypair.json)\"" > target/bench/groth16_bench-pubkey.json
shasum -a 256 target/deploy/paranoid.so target/deploy-localnet/paranoid.so
