#!/usr/bin/env bash
# Compiles the request circuit and runs a LOCAL single-party Groth16 setup (BN254).
# Whoever runs this knows the toxic waste and could forge proofs: before launch the operator
# replaces it with a multi-party phase 2 ceremony (see CORE.md). Nothing is downloaded.
#   build/request_js/request.wasm   witness generator (client)
#   build/request.zkey              proving key (client)
#   build/verification_key.json     verifying key (gateway)
set -euo pipefail
cd "$(dirname "$0")/.."
if [ -f CEREMONY.md ] && [ "${FORCE_LOCAL_SETUP:-}" != "1" ]; then
  echo "build/request.zkey comes from the ceremony in CEREMONY.md; this script would replace it with a local one." >&2
  echo "Set FORCE_LOCAL_SETUP=1 only for a throwaway local build." >&2
  exit 1
fi
SNARKJS=node_modules/.bin/snarkjs
mkdir -p build
circom circuits/request.circom --O2 --r1cs --wasm --sym -o build
if [ ! -f build/pot15_final.ptau ]; then
  $SNARKJS powersoftau new bn128 15 build/pot15_0.ptau
  $SNARKJS powersoftau contribute build/pot15_0.ptau build/pot15_1.ptau --name="paranoid local" -e="$(head -c 64 /dev/urandom | base64)"
  $SNARKJS powersoftau prepare phase2 build/pot15_1.ptau build/pot15_final.ptau
  rm build/pot15_0.ptau build/pot15_1.ptau
fi
$SNARKJS groth16 setup build/request.r1cs build/pot15_final.ptau build/request_0.zkey
$SNARKJS zkey contribute build/request_0.zkey build/request.zkey --name="paranoid local" -e="$(head -c 64 /dev/urandom | base64)"
rm build/request_0.zkey
$SNARKJS zkey export verificationkey build/request.zkey build/verification_key.json
$SNARKJS r1cs info build/request.r1cs
shasum -a 256 build/request_js/request.wasm build/request.zkey build/verification_key.json
