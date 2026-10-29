// What it would cost to verify a PARANOID request proof on-chain (bench/, never deployed).
// The product never needs this: gateways verify off-chain against roots the program stores,
// and slashing needs only the revealed secret. Measured so the choice is made on numbers.
import { after, test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { ComputeBudgetProgram, Keypair, PublicKey, Transaction, TransactionInstruction, sendAndConfirmTransaction } from "@solana/web3.js";
import { MemberTree, commitmentOf, leafOf, loadHash, messageHash, proveRequest, randomSecret, releaseProver } from "../sdk/rln.ts";
import { inputBytes, proofBytes } from "../tools/groth16-bytes.mjs";
import { airdrop, connection } from "./harness.ts";

const BENCH = new PublicKey(JSON.parse(readFileSync("target/bench/groth16_bench-pubkey.json", "utf8")));
after(releaseProver);

test("one request proof verifies on-chain; a tampered one is rejected", async () => {
  const hash = await loadHash();
  const secret = randomSecret();
  const tree = new MemberTree(hash);
  tree.set(0, leafOf(hash, commitmentOf(hash, secret), 9, 300));
  const x = await messageHash(new TextEncoder().encode("bench"));
  const { proof, publicSignals } = await proveRequest(
    { secret, endEpoch: 9, questions: 300, leafIndex: 0, tree, epoch: 3, start: 0, weight: 4, x, limit: 1000 },
    { wasm: "build/request_js/request.wasm", zkey: "build/request.zkey" },
  );
  const payer = Keypair.generate();
  await airdrop(payer.publicKey);
  const run = async (data: Buffer) => {
    const tx = new Transaction()
      .add(ComputeBudgetProgram.setComputeUnitLimit({ units: 1_400_000 }))
      .add(new TransactionInstruction({ programId: BENCH, keys: [], data }));
    const sig = await sendAndConfirmTransaction(connection, tx, [payer], { commitment: "confirmed" });
    const meta = (await connection.getTransaction(sig, { commitment: "confirmed", maxSupportedTransactionVersion: 0 }))!.meta!;
    return { cu: meta.computeUnitsConsumed!, size: tx.serialize().length };
  };
  const good = Buffer.concat([proofBytes(proof), inputBytes(publicSignals)]);
  const { cu, size } = await run(good);
  const bad = Buffer.from(good);
  bad[256 + 32 * 9 - 1] ^= 1;
  await assert.rejects(run(bad));
  const line = `groth16 verify on-chain (13 public signals): ${cu} CU, transaction ${size} bytes`;
  writeFileSync("target/cu-verify.txt", line + "\n");
  console.log(line);
  assert.ok(cu < 1_400_000);
});
