// Paying for questions with the PARANOID token, on our own validator holding the real mainnet
// PARANOID mint, its Meteora DAMM v2 SOL pool and Pyth's SOL/USD update (fixtures/accounts).
// The price is checked against an independent bigint computation from the same account bytes.
import { before, test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { Keypair, PublicKey } from "@solana/web3.js";
import { getAccount, getAssociatedTokenAddressSync, getMint } from "@solana/spl-token";
import { currentRoot, siblings } from "../sdk/chain.ts";
import { toBytes32 } from "../sdk/rln.ts";
import {
  A, BN, airdrop, authority, config, connection, ensureInitialized, expectError, mirror, newUser, program, RATE, report, send, treeState, usdcAccount,
} from "./harness.ts";
import { PROGRAM_ID } from "../sdk/chain.ts";

const MINT = new PublicKey("22rWiyDiY9GgxSrFiWC2vX1GxNxjkvrRPmZJDzv9pq6X");
const POOL = new PublicKey("5Diq3hfJ6QTcJXhAG1XVBiSLUQJHNue24YkkzLKmJooH");
const PYTH = new PublicKey("7UVimffxr9ow1uXYxsr4LHAcV58mLzhmwaeKvJ1pjLiE");
const RAYDIUM_STANDIN_POOL = new PublicKey("8sZWRu9KkzGj6GMozqmYffqzSKweZpXXXShQqZBA5bnV");
const PREMIUM = 1_000;
const programData = PublicKey.findProgramAddressSync([PROGRAM_ID.toBuffer()], new PublicKey("BPFLoaderUpgradeab1e11111111111111111111111"))[0];
const tokenPay = PublicKey.findProgramAddressSync([Buffer.from("token_pay")], PROGRAM_ID)[0];
const holder = Keypair.fromSecretKey(Uint8Array.from(JSON.parse(readFileSync("fixtures/token-holder.json", "utf8"))));
const holderAta = getAssociatedTokenAddressSync(MINT, holder.publicKey);

/** Independent price: the same formula in bigints, from the raw account bytes. */
async function expectedCost(questions: number, premium = PREMIUM): Promise<bigint> {
  const pool = (await connection.getAccountInfo(POOL))!.data;
  const pyth = (await connection.getAccountInfo(PYTH))!.data;
  const sqrt = pool.readBigUInt64LE(456) + (pool.readBigUInt64LE(464) << 64n);
  const paranoidIsA = new PublicKey(pool.subarray(168, 200)).equals(MINT);
  const price = pyth.readBigInt64LE(41 + 32);
  const expo = pyth.readInt32LE(41 + 48);
  const usdc = RATE * BigInt(questions);
  const n = usdc * (10_000n + BigInt(premium)) * 1_000n * 10n ** BigInt(-expo);
  const ceil = (a: bigint, b: bigint) => (a + b - 1n) / b;
  const q64 = 1n << 64n;
  const t = paranoidIsA ? ceil(ceil(n * q64, sqrt) * q64, sqrt) : ceil(ceil(n * sqrt, q64) * sqrt, q64);
  return ceil(t, price * 10_000n);
}

const setIx = (signer: Keypair, pool = POOL, priceUpdate = PYTH, premium = PREMIUM) =>
  program.methods.setTokenPayment(premium).accountsPartial({ authority: signer.publicKey, programData, mint: MINT, pool, priceUpdate }).signers([signer]);

const depositTokenIx = (commitment: bigint, member: PublicKey, questions: number, max: bigint) =>
  program.methods
    .depositToken(Array.from(toBytes32(commitment)), questions, new BN(max.toString()))
    .accountsPartial({ depositor: holder.publicKey, member, mint: MINT, source: holderAta, pool: POOL, priceUpdate: PYTH });

before(async () => {
  await ensureInitialized(await usdcAccount(Keypair.generate().publicKey));
  await airdrop(holder.publicKey);
});

test("set_token_payment: upgrade authority only, real pool and Pyth update only, premium capped, once", async () => {
  const stranger = Keypair.generate();
  await airdrop(stranger.publicKey);
  await expectError(setIx(stranger).rpc(), "NotUpgradeAuthority");
  await expectError(setIx(authority, POOL, PYTH, 5_001).rpc(), "InvalidParameter");
  await expectError(setIx(authority, POOL, PYTH, -5_001).rpc(), "InvalidParameter");
  await expectError(setIx(authority, RAYDIUM_STANDIN_POOL).rpc(), "TokenPayInvalid");
  await expectError(setIx(authority, POOL, POOL).rpc(), "PriceInvalid");
  await send("set_token_payment", setIx(authority));
  const tp = await program.account.tokenPay.fetch(tokenPay);
  assert.ok(tp.mint.equals(MINT) && tp.pool.equals(POOL) && tp.priceUpdate.equals(PYTH));
  assert.equal(tp.premiumBps, PREMIUM);
  await assert.rejects(setIx(authority).rpc(), "a second call finds the account in use");
});

test("deposit_token: burns exactly the quoted tokens, joins the tree, moves no USDC", async () => {
  const u = await newUser(0n);
  const questions = 500; // the local launch constants cap a membership at 1,000 tickets
  const cost = await expectedCost(questions);
  const usd = Number(cost) / 1e9 * 2.7727e-5;
  console.log(`quote: ${questions} questions = ${(Number(cost) / 1e9).toLocaleString()} PARANOID (~$${usd.toFixed(2)} at the clone's price)`);

  await expectError(depositTokenIx(u.commitment, u.member, questions, cost - 1n).signers([holder]).rpc(), "CostAboveMax");

  const before = (await getAccount(connection, holderAta)).amount;
  const supply0 = (await getMint(connection, MINT)).supply;
  const cfg0 = await config();
  await send("deposit_token", depositTokenIx(u.commitment, u.member, questions, cost), [holder]);

  assert.equal(before - (await getAccount(connection, holderAta)).amount, cost, "the holder paid exactly the quote");
  assert.equal(supply0 - (await getMint(connection, MINT)).supply, cost, "every token paid left the supply");
  const m = await program.account.member.fetch(u.member);
  assert.equal(Number(m.questions.toString()), questions);
  assert.ok(m.depositor.equals(holder.publicKey));
  const cfg1 = await config();
  assert.equal(cfg1.deposited.toString(), cfg0.deposited.toString(), "no USDC accounted");
  assert.equal(Number(cfg1.members) - Number(cfg0.members), 1);
  const tp = await program.account.tokenPay.fetch(tokenPay);
  assert.equal(tp.questionsSold.toString(), String(questions));
  assert.equal(tp.tokensBurned.toString(), cost.toString());
  const tree = await mirror();
  assert.equal(tree.root(), currentRoot(await treeState()), "the client's tree matches the chain");

  // top up the same membership with tokens
  const add = 250;
  const addCost = await expectedCost(add);
  const path = siblings(tree, m.leafIndex);
  const b1 = (await getAccount(connection, holderAta)).amount;
  await send(
    "top_up_token",
    program.methods.topUpToken(add, path, new BN(addCost.toString())).accountsPartial({ depositor: holder.publicKey, member: u.member, mint: MINT, source: holderAta, pool: POOL, priceUpdate: PYTH }),
    [holder],
  );
  assert.equal(b1 - (await getAccount(connection, holderAta)).amount, addCost);
  assert.equal(Number((await program.account.member.fetch(u.member)).questions.toString()), questions + add);

  // someone else's tokens cannot pay
  const other = await newUser(0n);
  await expectError(
    program.methods.depositToken(Array.from(toBytes32(other.commitment)), 250, new BN("999999999999999999"))
      .accountsPartial({ depositor: other.wallet.publicKey, member: other.member, mint: MINT, source: holderAta, pool: POOL, priceUpdate: PYTH })
      .signers([other.wallet]).rpc(),
    "ConstraintTokenOwner",
  );
  console.log(report());
});

test("set_token_premium: upgrade authority only, capped, a discount is what the buyer then pays", async () => {
  const premiumIx = (signer: Keypair, bps: number) =>
    program.methods.setTokenPremium(bps).accountsPartial({ authority: signer.publicKey, programData }).signers([signer]);
  const stranger = Keypair.generate();
  await airdrop(stranger.publicKey);
  await expectError(premiumIx(stranger, -1_000).rpc(), "NotUpgradeAuthority");
  await expectError(premiumIx(authority, -5_001).rpc(), "InvalidParameter");
  await expectError(premiumIx(authority, 5_001).rpc(), "InvalidParameter");
  await send("set_token_premium", premiumIx(authority, -1_000));
  assert.equal((await program.account.tokenPay.fetch(tokenPay)).premiumBps, -1_000);

  const questions = 250;
  const cost = await expectedCost(questions, -1_000);
  assert.ok(cost < await expectedCost(questions, PREMIUM), "10% off costs fewer tokens than 10% on top");
  const u = await newUser(0n);
  await expectError(depositTokenIx(u.commitment, u.member, questions, cost - 1n).signers([holder]).rpc(), "CostAboveMax");
  const before = (await getAccount(connection, holderAta)).amount;
  await send("deposit_token", depositTokenIx(u.commitment, u.member, questions, cost), [holder]);
  assert.equal(before - (await getAccount(connection, holderAta)).amount, cost, "the holder paid exactly the discounted quote");

  await send("set_token_premium", premiumIx(authority, PREMIUM));
  assert.equal((await program.account.tokenPay.fetch(tokenPay)).premiumBps, PREMIUM);
  console.log(report());
});
