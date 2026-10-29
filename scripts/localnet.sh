#!/usr/bin/env bash
# Own local validator for PARANOID: `scripts/localnet.sh up` / `down`.
# RPC on $PARANOID_RPC_PORT (default 18929). Loads the localnet build of the program as an
# upgradeable program (upgrade authority fixtures/test-authority.json, so `initialize` runs as
# on mainnet), the mainnet Meteora DAMM v2 and Raydium CP-Swap binaries, and every account in
# fixtures/accounts. Pyth's SOL/USD update is loaded with today's publish time, so the price checks
# keep passing however old the snapshot is.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"
PORT="${PARANOID_RPC_PORT:-18929}"
LEDGER=target/test-ledger
PIDFILE=target/validator.pid
case "${1:-}" in
  up)
    if lsof -iTCP:"$PORT" -sTCP:LISTEN >/dev/null; then echo "port $PORT is taken; set PARANOID_RPC_PORT"; exit 1; fi
    rm -rf "$LEDGER"
    args=(--ledger "$LEDGER" --rpc-port "$PORT" --faucet-port $((PORT + 10)) --gossip-port $((PORT + 20))
      --dynamic-port-range $((PORT + 30))-$((PORT + 60)) --quiet --reset --limit-ledger-size 200000000
      --upgradeable-program 9JJqzEoDGkTE8CMLATRw7CqFhcyWVegfhFuEeGfBfVGm target/deploy-localnet/paranoid.so fixtures/test-authority.json
      --bpf-program CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C fixtures/programs/raydium_cpmm.so
      --bpf-program cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG fixtures/programs/meteora_damm_v2.so)
    mkdir -p target
    node -e '
const fs=require("fs");const raw=fs.readFileSync(process.argv[1],"utf8");
const b64=JSON.parse(raw).account.data[0];const d=Buffer.from(b64,"base64");
const now=BigInt(Math.floor(Date.now()/1000));d.writeBigInt64LE(now,93);d.writeBigInt64LE(now,101);
fs.writeFileSync(process.argv[2],raw.replace(b64,d.toString("base64")));' fixtures/accounts/pyth_sol_usd.json target/pyth_sol_usd.json
    for f in fixtures/accounts/*.json; do
      [ "$f" = fixtures/accounts/pyth_sol_usd.json ] && f=target/pyth_sol_usd.json
      args+=(--account - "$f")
    done
    # CU bench for on-chain proof verification (bench/), loaded when built.
    if [ -f target/bench/groth16_bench.so ]; then
      args+=(--bpf-program "$(solana-keygen pubkey target/bench/groth16_bench-keypair.json)" target/bench/groth16_bench.so)
    fi
    nohup solana-test-validator "${args[@]}" > target/validator.log 2>&1 &
    echo $! > "$PIDFILE"
    for _ in $(seq 1 60); do
      if solana -u "http://127.0.0.1:$PORT" cluster-version >/dev/null 2>&1; then echo "validator up on $PORT (pid $(cat $PIDFILE))"; exit 0; fi
      sleep 1
    done
    echo "validator did not start; see target/validator.log"; exit 1 ;;
  down)
    if [ -f "$PIDFILE" ]; then kill "$(cat "$PIDFILE")" 2>/dev/null || true; rm -f "$PIDFILE"; echo "validator stopped"; fi ;;
  *) echo "usage: $0 up|down"; exit 2 ;;
esac
