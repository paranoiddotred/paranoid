#!/usr/bin/env bash
# Every test, in order: host unit and property tests (money rules, member tree), the circuit and
# gateway suite (no chain), then on a fresh own validator the program lifecycle, the solvency fuzz
# and the on-chain verification CU bench. Needs scripts/build.sh, scripts/setup-circuit.sh and
# fixtures (scripts/dump-fixtures.sh). Logs: target/test-logs. FUZZ_SEED / FUZZ_STEPS override.
set -uo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$HOME/.local/share/solana/install/active_release/bin:$PATH"
mkdir -p target/test-logs
fail=0
cargo test -p paranoid --lib > target/test-logs/unit.log 2>&1 || fail=1
node --test tests/circuit.test.ts > target/test-logs/circuit.log 2>&1 || fail=1
bash scripts/localnet.sh down >/dev/null
bash scripts/localnet.sh up > target/test-logs/validator.log 2>&1 || { cat target/test-logs/validator.log; exit 1; }
node --test tests/program.test.ts > target/test-logs/program.log 2>&1 || fail=1
node --test tests/fuzz.test.ts > target/test-logs/fuzz.log 2>&1 || fail=1
node --test tests/onchain-verify.test.ts > target/test-logs/verify.log 2>&1 || fail=1
node --test tests/token-pay.test.ts > target/test-logs/token-pay.log 2>&1 || fail=1
node --test tests/order.test.ts > target/test-logs/order.log 2>&1 || fail=1
node --test tests/stake.test.ts > target/test-logs/stake.log 2>&1 || fail=1
bash scripts/localnet.sh down >/dev/null
unit=$(grep -h '^test result' target/test-logs/unit.log | grep -o '[0-9]* passed' | awk '{s+=$1} END {print s+0}')
node_pass=$(grep -h '^ℹ pass' target/test-logs/{circuit,program,fuzz,verify,token-pay,order,stake}.log | awk '{s+=$3} END {print s+0}')
node_fail=$(grep -h '^ℹ fail' target/test-logs/{circuit,program,fuzz,verify,token-pay,order,stake}.log | awk '{s+=$3} END {print s+0}')
for f in circuit program fuzz verify token-pay order stake; do printf "%-8s %s\n" "$f" "$(grep -h '^ℹ pass\|^ℹ fail' target/test-logs/$f.log | tr '\n' ' ')"; done
grep -h "fuzz seed\|groth16 verify" target/test-logs/*.log || true
cat target/cu-program.txt 2>/dev/null
echo "unit ${unit}, node ${node_pass}: $((unit + node_pass)) passed, ${node_fail} failed"
exit $fail
