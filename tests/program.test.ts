// The whole life of the program (v2, question packs) on our own validator: launch, reconfigure
// before anyone joins, buying tickets, top-ups, requests proven against the on-chain root, a ticket
// spent twice on different days, slashing, the treasury sweep and the burn crank on a snapshot of the
// live Meteora PARANOID/SOL pool. Every error path has its own case.
import { after, before, test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { Keypair, PublicKey, SystemProgram, Transaction } from "@solana/web3.js";
import {
  createAccount, createAssociatedTokenAccountInstruction, createMint, createSyncNativeInstruction,
  getAssociatedTokenAddressSync, getMint, TOKEN_PROGRAM_ID, transfer,
} from "@solana/spl-token";
import { acceptedRoots, anonymitySet, currentRoot, siblings, PROGRAM_ID } from "../sdk/chain.ts";
import {
  MemberTree, NullifierBook, messageHash, proveRequest, readPublic, releaseProver, toBytes32, verifyRequest,
} from "../sdk/rln.ts";
import {
  A, BN, PARAMS, RATE, airdrop, authority, balance, config, depositIx, epochNow, expectError, freshEpoch,
  hash, members, mirror, newUser, program, report, send, topUpIx, treeState, usdcAccount, waitForEpoch, type User,
} from "./harness.ts";

const artifacts = { wasm: "build/request_js/request.wasm", zkey: "build/request.zkey" };
const vkey = JSON.parse(readFileSync("build/verification_key.json", "utf8"));
const enc = new TextEncoder();
const LOADER = new PublicKey("BPFLoaderUpgradeab1e11111111111111111111111");
const programData = PublicKey.findProgramAddressSync([program.programId.toBuffer()], LOADER)[0];
const POOL = new PublicKey("8sZWRu9KkzGj6GMozqmYffqzSKweZpXXXShQqZBA5bnV");
const STANDIN = new PublicKey("8sdE4z8X8DZNjFSXW6fJt66339xLGmBA9Q1h8zxMpXKt");
const CPMM = new PublicKey("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C");

let treasury: PublicKey;
const users: Record<string, User> = {};

async function assertMirror() {
  const m = await mirror();
  assert.equal(m.root(), currentRoot(await treeState()), "SDK mirror and on-chain root differ");
  return m;
}

/** Proves with the member's leaf as it is on chain now, unless `as` pins an older end day and count. */
async function request(u: User, epoch: number, start: number, weight: number, body: string, tree?: MemberTree, as?: { endEpoch: number; questions: number }) {
  const t = tree ?? (await mirror());
  const m = await program.account.member.fetch(u.member);
  const leaf = as ?? { endEpoch: m.endEpoch, questions: Number(m.questions.toString()) };
  return proveRequest(
    { secret: u.secret, ...leaf, leafIndex: m.leafIndex, tree: t, epoch, start, weight, x: await messageHash(enc.encode(body)), limit: PARAMS.limit },
    artifacts,
  );
}

async function gatewayCheck(p: Awaited<ReturnType<typeof request>>, body: string, epoch: number) {
  return verifyRequest(vkey, p, {
    acceptedRoots: acceptedRoots(await treeState(), epoch), epoch, limit: PARAMS.limit, x: await messageHash(enc.encode(body)),
  });
}

before(async () => {
  treasury = await usdcAccount(Keypair.generate().publicKey);
  for (const name of ["alice", "bob", "carol", "dave", "erin"]) users[name] = await newUser();
});

after(async () => {
  await releaseProver();
  writeFileSync("target/cu-program.txt", report() + "\n");
  console.log(report());
});

const reconfigureIx = (params: typeof PARAMS, signer = authority) =>
  program.methods.reconfigure(params).accountsPartial({ authority: signer.publicKey, programData }).signers([signer]);

test("initialize: only the upgrade authority, only valid launch constants, only once", async () => {
  const stranger = Keypair.generate();
  await airdrop(stranger.publicKey);
  const init = (params: typeof PARAMS, signer = authority, t = treasury) =>
    program.methods.initialize(params).accountsPartial({ authority: signer.publicKey, programData, treasury: t }).signers([signer]);
  await expectError(init(PARAMS, stranger).rpc(), "NotUpgradeAuthority");
  for (const bad of [{ epochSeconds: 1 }, { rate: new BN(0) }, { limit: 0 }, { maxEpochs: 0 }, { minQuestions: 0 },
    { minQuestions: PARAMS.limit + 1 }, { burnBps: 10_001 }, { slashBurnBps: 10_001 }, { rate: new BN("18446744073709551615"), limit: 2, minQuestions: 1 }]) {
    await expectError(init({ ...PARAMS, ...bad }).rpc(), "InvalidParameter");
  }
  const otherMint = await createMint(provider().connection, authority, authority.publicKey, null, 6);
  const wrongTreasury = await createAccount(provider().connection, authority, otherMint, authority.publicKey);
  await expectError(init(PARAMS, authority, wrongTreasury).rpc(), "InvalidTreasury");

  await send("initialize", init(PARAMS));
  await expectError(init(PARAMS).rpc(), "already in use");
  const c = await config();
  assert.equal(c.treasury.toBase58(), treasury.toBase58());
  assert.equal(c.minQuestions, PARAMS.minQuestions);
  assert.equal(c.burnMint.toBase58(), PublicKey.default.toBase58());
  const tree = await treeState();
  assert.equal(currentRoot(tree), new MemberTree(hash).root());
  assert.equal(tree.nextIndex, 0);
});

function provider() {
  return program.provider as any;
}

test("reconfigure: upgrade authority only, valid constants only, and it restarts the day count", async () => {
  const stranger = Keypair.generate();
  await airdrop(stranger.publicKey);
  await expectError(reconfigureIx(PARAMS, stranger).rpc(), "NotUpgradeAuthority");
  await expectError(reconfigureIx({ ...PARAMS, minQuestions: 0 }).rpc(), "InvalidParameter");
  const g0 = (await config()).genesis.toNumber();
  await new Promise((r) => setTimeout(r, 1200));
  await send("reconfigure", reconfigureIx({ ...PARAMS, rate: new BN(5_000), limit: 500 }));
  let c = await config();
  assert.equal(BigInt(c.rate.toString()), 5_000n);
  assert.equal(c.limit, 500);
  assert.ok(c.genesis.toNumber() >= g0);
  await send("reconfigure", reconfigureIx(PARAMS));
  c = await config();
  assert.equal(BigInt(c.rate.toString()), RATE);
  assert.equal(c.limit, PARAMS.limit);
});

test("deposit: too few or too many questions, bad commitments and foreign USDC are refused", async () => {
  const u = users.alice;
  await expectError(depositIx(u, PARAMS.minQuestions - 1).signers([u.wallet]).rpc(), "InvalidQuestions");
  await expectError(depositIx(u, PARAMS.limit + 1).signers([u.wallet]).rpc(), "InvalidQuestions");
  const zero = { ...u, commitment: 0n, member: A.member(0n) };
  await expectError(depositIx(zero, 300).signers([u.wallet]).rpc(), "InvalidCommitment");
  const tooBig = (1n << 256n) - 1n;
  await expectError(depositIx({ ...u, commitment: tooBig, member: A.member(tooBig) }, 300).signers([u.wallet]).rpc(), "InvalidCommitment");
  await expectError(depositIx({ ...u, usdc: users.bob.usdc }, 300).signers([u.wallet]).rpc(), "ConstraintTokenOwner");
});

test("deposit: pays rate x questions straight to the buyback (burn vault), holds nothing, adds the leaf the client expects", async () => {
  const e = await freshEpoch();
  const t0 = await balance(A.treasuryVault);
  const b0 = await balance(A.burnVault);
  const v0 = await balance(A.vault);
  const plan: [string, number][] = [["alice", 250], ["bob", 400], ["carol", 300], ["dave", 250]];
  for (const [name, questions] of plan) {
    const u = users[name];
    const usdc0 = await balance(u.usdc);
    await send("deposit", depositIx(u, questions), [u.wallet]);
    const m = await program.account.member.fetch(u.member);
    assert.equal(m.startEpoch, e);
    assert.equal(m.endEpoch, e + PARAMS.maxEpochs - 1);
    assert.equal(Number(m.questions.toString()), questions);
    assert.deepEqual(Object.keys(m.status), ["active"]);
    assert.equal(usdc0 - (await balance(u.usdc)), RATE * BigInt(questions));
    await assertMirror();
  }
  const paid = RATE * 1_200n;
  assert.equal((await balance(A.burnVault)) - b0, paid); // all of it buys back since 2026-10-27
  assert.equal((await balance(A.treasuryVault)) - t0, 0n);
  assert.equal(await balance(A.vault), v0, "nothing is held for members");
  await expectError(depositIx(users.alice, 300).signers([users.alice.wallet]).rpc(), "already in use");
  const c = await config();
  assert.equal(c.members.toNumber(), 4);
  assert.equal(BigInt(c.deposited.toString()), paid);
  assert.equal(BigInt(c.paidTreasury.toString()) + BigInt(c.paidBurn.toString()), paid);
  assert.equal(anonymitySet(await members(), e), 4);
  await expectError(reconfigureIx(PARAMS).rpc(), "AlreadyInUse");
});

test("a request proven against the on-chain root passes the gateway", async () => {
  const e = await freshEpoch(4);
  const body = '{"model":"llama","messages":[{"role":"user","content":"hello"}]}';
  const p = await request(users.alice, e, 0, 2, body);
  const v = await gatewayCheck(p, body, e);
  assert.equal(v.ok, true);
  assert.equal(new NullifierBook().record(readPublic(p.publicSignals)).status, "fresh");
  await assert.rejects(request(users.alice, e, 249, 2, "past the last ticket bought"));
});

test("top-up: depositor only, valid path only, within min and cap; more tickets and a fresh year", async () => {
  const u = users.alice;
  const e = await freshEpoch(5);
  const tree = await assertMirror();
  const m = await program.account.member.fetch(u.member);
  const path = siblings(tree, m.leafIndex);
  const oldLeaf = { endEpoch: m.endEpoch, questions: Number(m.questions.toString()) };
  const proofOnOldLeaf = await request(u, e, 10, 1, "asked before the top-up", tree, oldLeaf);
  const bobsPath = { ...users.bob, member: u.member };
  await expectError(topUpIx(bobsPath, 300, path).signers([users.bob.wallet]).rpc(), "NotDepositor");
  const wrong = path.map((s, i) => (i === 5 ? Array.from(toBytes32(7n)) : s));
  await expectError(topUpIx(u, 300, wrong).signers([u.wallet]).rpc(), "InvalidPath");
  await expectError(topUpIx(u, PARAMS.minQuestions - 1, path).signers([u.wallet]).rpc(), "InvalidQuestions");
  await expectError(topUpIx(u, PARAMS.limit - 249, path).signers([u.wallet]).rpc(), "InvalidQuestions");

  const t0 = await balance(A.treasuryVault);
  const b0 = await balance(A.burnVault);
  await send("top_up", topUpIx(u, 250, path), [u.wallet]);
  const after = await program.account.member.fetch(u.member);
  assert.equal(Number(after.questions.toString()), 500);
  assert.equal(after.endEpoch, (await epochNow()) + PARAMS.maxEpochs - 1);
  assert.equal(after.leafIndex, m.leafIndex);
  const paid = RATE * 250n;
  assert.equal((await balance(A.burnVault)) - b0, paid); // all of it buys back since 2026-10-27
  assert.equal((await balance(A.treasuryVault)) - t0, 0n);
  await assertMirror();

  const state = await treeState();
  assert.ok(acceptedRoots(state, e).has(tree.root()), "the old leaf only proves fewer tickets, so its roots stay good");
  assert.equal((await gatewayCheck(proofOnOldLeaf, "asked before the top-up", e)).ok, true);
  const fresh = await request(u, e, 496, 4, "the last four tickets after the top-up");
  assert.equal((await gatewayCheck(fresh, "the last four tickets after the top-up", e)).ok, true);
  await assert.rejects(request(u, e, 497, 4, "one past the new count"));
});

test("a ticket never comes back: reused on a later day, the secret falls out and anyone can slash", async () => {
  const u = users.carol;
  const e = await freshEpoch(4);
  const tree = await assertMirror();
  const m = await program.account.member.fetch(u.member);
  const book = new NullifierBook();
  const p1 = await request(u, e, 3, 1, "first message", tree);
  assert.equal(book.record(readPublic(p1.publicSignals)).status, "fresh");
  await waitForEpoch(e + 1);
  const p2 = await request(u, e + 1, 2, 2, "next day, reuses ticket 3", tree);
  assert.equal((await gatewayCheck(p2, "next day, reuses ticket 3", e + 1)).ok, true);
  const verdict = book.record(readPublic(p2.publicSignals));
  assert.equal(verdict.status, "double");
  const secret = verdict.status === "double" ? verdict.secret : 0n;
  assert.equal(secret, u.secret);

  const reporter = Keypair.generate();
  await airdrop(reporter.publicKey);
  const slashIx = (s: bigint, sib = siblings(tree, m.leafIndex)) =>
    program.methods.slash(Array.from(toBytes32(s)), sib)
      .accountsPartial({ reporter: reporter.publicKey, member: u.member }).signers([reporter]);
  await expectError(slashIx(secret + 1n).rpc(), "SecretMismatch");
  await expectError(slashIx(secret, siblings(new MemberTree(hash), 0)).rpc(), "InvalidPath");

  const t0 = await balance(A.treasuryVault);
  const b0 = await balance(A.burnVault);
  const r0 = await provider().connection.getBalance(reporter.publicKey);
  await send("slash", slashIx(secret));
  const after = await program.account.member.fetch(u.member);
  assert.deepEqual(Object.keys(after.status), ["slashed"]);
  assert.equal(await balance(A.treasuryVault), t0, "a slash moves no money: it was all paid at purchase");
  assert.equal(await balance(A.burnVault), b0);
  assert.ok((await provider().connection.getBalance(reporter.publicKey)) <= r0, "the reporter is paid nothing");

  const now = await epochNow();
  const state = await treeState();
  assert.ok(!acceptedRoots(state, now).has(tree.root()), "roots holding the slashed leaf are retired at once");
  assert.ok(acceptedRoots(state, now).has(currentRoot(state)));
  await assertMirror();
  await expectError(slashIx(secret).rpc(), "AlreadySlashed");
  await expectError(topUpIx(u, 300, siblings(await mirror(), m.leafIndex)).signers([u.wallet]).rpc(), "NotActive");
  // The secret is public now, but no root the gateway accepts holds its leaf any more.
  await assert.rejects(request(u, now, 5, 1, "free ride with a public secret"));
  assert.equal(anonymitySet(await members(), now), 3);
});

test("a membership past its last day proves nothing; a top-up brings it back for a full year", async () => {
  const u = users.erin;
  const e = await freshEpoch(5);
  await send("deposit", depositIx(u, 250), [u.wallet]);
  const m = await program.account.member.fetch(u.member);
  await assert.rejects(request(u, m.endEpoch + 1, 0, 1, "after the last day"));
  await send("top_up", topUpIx(u, 250, siblings(await mirror(), m.leafIndex)), [u.wallet]);
  const after = await program.account.member.fetch(u.member);
  assert.equal(Number(after.questions.toString()), 500);
  assert.ok(after.endEpoch >= e + PARAMS.maxEpochs - 1);
  const p = await request(u, e, 300, 1, "a ticket bought in the top-up");
  assert.equal((await gatewayCheck(p, "a ticket bought in the top-up", e)).ok, true);
});

test("sweep: anyone moves the treasury vault to the launch-fixed treasury, and only there", async () => {
  // purchases no longer pay the treasury; what reaches it from before (or from anyone) still sweeps out
  await transfer(provider().connection, users.alice.wallet, users.alice.usdc, A.treasuryVault, users.alice.wallet, 1_000_000n);
  const held = await balance(A.treasuryVault);
  assert.ok(held > 0n);
  const before = await balance(treasury);
  const sweep = (to: PublicKey) => program.methods.sweepTreasury().accountsPartial({ treasury: to });
  await expectError(sweep(users.alice.usdc).rpc(), "ConstraintHasOne");
  await send("sweep_treasury", sweep(treasury));
  assert.equal((await balance(treasury)) - before, held);
  assert.equal(await balance(A.treasuryVault), 0n);
  const c = await config();
  assert.equal(BigInt(c.swept.toString()), held);
  assert.equal(BigInt(c.paidTreasury.toString()), 0n);
  await send("sweep_treasury (empty)", sweep(treasury));
});

test("burn: target set once; USDC leaves at the oracle price; the swap buys in the pool and burns", async () => {
  const MINT = new PublicKey("22rWiyDiY9GgxSrFiWC2vX1GxNxjkvrRPmZJDzv9pq6X");
  const DAMM_POOL = new PublicKey("5Diq3hfJ6QTcJXhAG1XVBiSLUQJHNue24YkkzLKmJooH");
  const PYTH = new PublicKey("7UVimffxr9ow1uXYxsr4LHAcV58mLzhmwaeKvJ1pjLiE");
  const WSOL = new PublicKey("So11111111111111111111111111111111111111112");
  const poolData = (await provider().connection.getAccountInfo(DAMM_POOL))!.data;
  const poolVaultA = new PublicKey(poolData.subarray(232, 264));
  const poolVaultB = new PublicKey(poolData.subarray(264, 296));

  const setTarget = (signer: Keypair, mint = MINT, pool = DAMM_POOL) => program.methods.setBurnTarget()
    .accountsPartial({ authority: signer.publicKey, programData, burnMint: mint, wsolMint: WSOL, pool, priceUpdate: PYTH, burnTokenProgram: TOKEN_PROGRAM_ID }).signers([signer]);
  const stranger = Keypair.generate();
  await airdrop(stranger.publicKey);
  await expectError(setTarget(stranger).rpc(), "NotUpgradeAuthority");
  await expectError(setTarget(authority, STANDIN).rpc(), "BurnPoolInvalid");
  await expectError(setTarget(authority, MINT, POOL).rpc(), "TokenPayInvalid"); // a CP-Swap pool is not a DAMM pool
  await send("set_burn_target", setTarget(authority));
  await expectError(setTarget(authority).rpc(), "already in use");
  assert.equal((await config()).burnPool.toBase58(), DAMM_POOL.toBase58());

  // A keeper brings wrapped SOL and takes vault USDC at the oracle price, keeping 0.3%.
  const keeper = Keypair.generate();
  await airdrop(keeper.publicKey);
  const keeperWsol = getAssociatedTokenAddressSync(WSOL, keeper.publicKey);
  const keeperUsdc = await usdcAccount(keeper.publicKey);
  const wrap = new Transaction().add(
    createAssociatedTokenAccountInstruction(keeper.publicKey, keeperWsol, keeper.publicKey, WSOL),
    SystemProgram.transfer({ fromPubkey: keeper.publicKey, toPubkey: keeperWsol, lamports: 2_000_000_000 }),
    createSyncNativeInstruction(keeperWsol),
  );
  await provider().sendAndConfirm(wrap, [keeper]);

  const funded = await balance(A.burnVault);
  assert.ok(funded > 0n);
  const convert = (usdc: bigint) => program.methods.convertBurn(new BN(usdc.toString()))
    .accountsPartial({ caller: keeper.publicKey, callerUsdc: keeperUsdc, callerWsol: keeperWsol, priceUpdate: PYTH }).signers([keeper]);
  await expectError(convert(0n).rpc(), "BurnAmountInvalid");
  await expectError(convert(funded + 1_000_000n).rpc(), "BurnAmountInvalid");
  await expectError(convert(26_000_000n).rpc(), "BurnAmountInvalid");

  const amount = funded < 25_000_000n ? funded : 25_000_000n;
  const usdcBefore = await balance(keeperUsdc);
  await send("convert_burn", convert(amount));
  assert.equal((await balance(keeperUsdc)) - usdcBefore, amount);
  assert.equal(await balance(A.burnVault), funded - amount);
  const gathered = await balance(A.wsolVault);
  // The independent price: lamports from the same Pyth bytes, less the keeper's 30 bps.
  const pyth = (await provider().connection.getAccountInfo(PYTH))!.data;
  const price = pyth.readBigInt64LE(73);
  const expo = pyth.readInt32LE(89);
  const fair = (amount * 1_000n * 10n ** BigInt(-expo)) / price;
  assert.equal(gathered, (fair * 9_970n) / 10_000n);

  // The swap spends the gathered wSOL in the real pool. Staking is open but nobody stakes yet, so it
  // burns what it bought (tests/stake.test.ts covers the share that goes to stakers).
  await send("open_stake", program.methods.openStake().accountsPartial({ payer: authority.publicKey }));
  await send("open_stake_reward", program.methods.openStakeReward().accountsPartial({ payer: authority.publicKey }));
  const supply0 = (await getMint(provider().connection, MINT)).supply;
  const crank = () => program.methods.crankBurn().accountsPartial({
    caller: keeper.publicKey, burnMint: MINT, wsolMint: WSOL, pool: DAMM_POOL, poolVaultA, poolVaultB,
    poolAuthority: new PublicKey("HLnpSz9h2S4hiLQ43rnSD9XkcUThA7B8hQMKmDaiTLcC"),
    eventAuthority: new PublicKey("3rmHSu74h1ZcmAisVcWerTCiRDQbUrBKmcwptYGjHfet"),
    dammProgram: new PublicKey("cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG"),
    burnTokenProgram: TOKEN_PROGRAM_ID,
  }).signers([keeper]);
  await send("crank_burn", crank());
  const supply1 = (await getMint(provider().connection, MINT)).supply;
  const c = await config();
  assert.ok(supply1 < supply0, "supply must fall");
  assert.equal(BigInt(c.tokenBurned.toString()), supply0 - supply1);
  assert.equal(BigInt(c.burnSpent.toString()), amount);
  assert.equal(await balance(A.bought), 0n);
  assert.equal(await balance(A.wsolVault), 0n, "a pool this deep takes the whole gathered amount");
  assert.equal((await program.account.stakeReward.fetch(PublicKey.findProgramAddressSync([Buffer.from("stake_reward")], PROGRAM_ID)[0])).given.toString(), "0", "nobody staked, nothing given");
  await expectError(crank().rpc(), "BurnTooSoon");
});
