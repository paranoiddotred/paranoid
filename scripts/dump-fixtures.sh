#!/usr/bin/env bash
# Snapshots the mainnet programs and accounts the local validator runs against.
# Meteora DAMM v2 with the live PARANOID/SOL pool and its two vaults, the PARANOID mint and Pyth's
# SOL/USD update, so the burn crank and token payments run on real pool and price state. A test holder
# (fixtures/token-holder.json) gets a made-up account with 10,000,000 PARANOID to pay with.
# Raydium CP-Swap with UTHR's USDC pool, the burn venue before the move to Meteora, stays as a pool of
# the wrong kind that the tests expect the program to refuse.
# USDC's mint authority is rewritten to a local test key (fixtures/usdc-authority.json) so tests
# can mint test USDC; every other byte of the mint is mainnet's.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"
RPC="${RPC:-https://api.mainnet-beta.solana.com}"

dump_program() { solana program dump -u "$RPC" "$1" "fixtures/programs/$2.so" >/dev/null; echo "program $2 $(shasum -a 256 "fixtures/programs/$2.so" | cut -c1-16)"; }
dump_account() { solana account -u "$RPC" "$1" --output json-compact > "fixtures/accounts/$2.json"; echo "account $2 $1"; sleep 0.4; }
field() { node -e '
const d=Buffer.from(JSON.parse(require("fs").readFileSync(process.argv[1])).account.data[0],"base64");
const o=+process.argv[2];const A="123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
let b=d.subarray(o,o+32),n=BigInt("0x"+b.toString("hex")),s="";while(n>0n){s=A[Number(n%58n)]+s;n/=58n}
let z=0;while(z<32&&b[z]===0)z++;console.log("1".repeat(z)+s)' "fixtures/accounts/$1.json" "$2"; }

dump_program CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C raydium_cpmm
dump_account EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v usdc_mint
dump_account 8sdE4z8X8DZNjFSXW6fJt66339xLGmBA9Q1h8zxMpXKt standin_mint
pool=8sZWRu9KkzGj6GMozqmYffqzSKweZpXXXShQqZBA5bnV
dump_account "$pool" pool
dump_account "$(field pool 8)" amm_config
dump_account "$(field pool 72)" pool_vault_0
dump_account "$(field pool 104)" pool_vault_1
dump_account "$(field pool 296)" observation

dump_program cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG meteora_damm_v2
dump_account 22rWiyDiY9GgxSrFiWC2vX1GxNxjkvrRPmZJDzv9pq6X paranoid_mint
dump_account 5Diq3hfJ6QTcJXhAG1XVBiSLUQJHNue24YkkzLKmJooH paranoid_sol_pool
dump_account "$(field paranoid_sol_pool 232)" damm_vault_a
dump_account "$(field paranoid_sol_pool 264)" damm_vault_b
dump_account 7UVimffxr9ow1uXYxsr4LHAcV58mLzhmwaeKvJ1pjLiE pyth_sol_usd

[ -f fixtures/token-holder.json ] || solana-keygen new -o fixtures/token-holder.json --no-bip39-passphrase -s >/dev/null
node -e '
const fs=require("fs");const {PublicKey}=require("@solana/web3.js");const {getAssociatedTokenAddressSync}=require("@solana/spl-token");
const holder=new PublicKey(process.argv[1]);const mint=new PublicKey("22rWiyDiY9GgxSrFiWC2vX1GxNxjkvrRPmZJDzv9pq6X");
const d=Buffer.alloc(165);mint.toBuffer().copy(d,0);holder.toBuffer().copy(d,32);
d.writeBigUInt64LE(10_000_000n*1_000_000_000n,64);d[108]=1;
fs.writeFileSync("fixtures/accounts/paranoid_holder_ata.json",`{"pubkey":"${getAssociatedTokenAddressSync(mint,holder).toBase58()}","account":{"lamports":2039280,"data":["${d.toString("base64")}","base64"],"owner":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA","executable":false,"rentEpoch":18446744073709551615,"space":165}}`);' "$(solana-keygen pubkey fixtures/token-holder.json)"
echo "account paranoid_holder_ata (made up, 10,000,000 PARANOID)"

[ -f fixtures/usdc-authority.json ] || solana-keygen new -o fixtures/usdc-authority.json --no-bip39-passphrase -s >/dev/null
node -e '
const fs=require("fs");const f="fixtures/accounts/usdc_mint.json";const raw=fs.readFileSync(f,"utf8");
const b64=JSON.parse(raw).account.data[0];const d=Buffer.from(b64,"base64");
const A="123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const key=process.argv[1];let n=0n;for(const c of key)n=n*58n+BigInt(A.indexOf(c));
Buffer.from(n.toString(16).padStart(64,"0"),"hex").copy(d,4);
fs.writeFileSync(f,raw.replace(b64,d.toString("base64")));' "$(solana-keygen pubkey fixtures/usdc-authority.json)"
echo "usdc mint authority -> $(solana-keygen pubkey fixtures/usdc-authority.json)"
