// Money and tree under random use (v2, question packs): purchases of random sizes, top-ups, slashes
// and treasury sweeps in random order, on our own validator. After every step nothing is held for
// members, the treasury and burn vault hold exactly what purchases paid them, every wallet's USDC adds
// up, ticket counts never pass the cap, and the SDK's tree mirror equals the chain's root.
import { after, test } from "node:test";
import assert from "node:assert/strict";
import { Keypair } from "@solana/web3.js";
import { currentRoot, siblings } from "../sdk/chain.ts";
import { releaseProver, toBytes32 } from "../sdk/rln.ts";
import {
  A, PARAMS, RATE, authority, balance, config, connection, depositIx, ensureInitialized, mirror,
  newUser, program, report, send, topUpIx, treeState, usdcAccount, type User,
} from "./harness.ts";

const SEED = Number(process.env.FUZZ_SEED ?? Date.now() % 1_000_000);
const STEPS = Number(process.env.FUZZ_STEPS ?? 120);
let state = SEED;
const rand = () => ((state = (state * 1103515245 + 12345) % 2147483648) / 2147483648);
const pick = <T>(xs: T[]): T => xs[Math.floor(rand() * xs.length)];
const between = (lo: number, hi: number) => lo + Math.floor(rand() * (hi - lo + 1));

after(async () => {
  await releaseProver();
  console.log(report());
});

test(`every purchase is accounted for exactly through ${STEPS} random steps (seed ${SEED})`, async () => {
  if (!(await connection.getAccountInfo(A.config))) await ensureInitialized(await usdcAccount(Keypair.generate().publicKey));
  const c0 = await config();
  const cap = c0.limit;
  const min = c0.minQuestions;
  const rate = BigInt(c0.rate.toString());
  const treasury = c0.treasury;
  const START = 1_000_000_000n;
  const users: User[] = [];
  for (let i = 0; i < 10; i++) users.push(await newUser(START));
  const spent = new Map<User, bigint>(users.map((u) => [u, 0n]));
  const held = new Map<User, number>();
  const slashed = new Set<User>();
  const counts: Record<string, number> = {};
  const base = {
    vault: await balance(A.vault),
    treasury: (await balance(A.treasuryVault)) + (await balance(treasury)),
    burn: await balance(A.burnVault),
    deposited: BigInt(c0.deposited.toString()),
  };
  let paidTreasury = 0n;
  let paidBurn = 0n;
  // Since 2026-10-26 half of every purchase burns and half goes to the treasury.
  const pay = (u: User, questions: number) => {
    const amount = rate * BigInt(questions);
    const burn = amount; // every purchase goes to the buyback since 2026-10-27
    spent.set(u, spent.get(u)! + amount);
    paidTreasury += amount - burn;
    paidBurn += burn;
  };

  for (let step = 0; step < STEPS; step++) {
    const fresh = users.filter((u) => !held.has(u));
    const active = users.filter((u) => held.has(u) && !slashed.has(u));
    const roll = rand();
    if (fresh.length && (roll < 0.35 || !active.length)) {
      const u = pick(fresh);
      const q = between(min, cap);
      await send("deposit", depositIx(u, q), [u.wallet]);
      held.set(u, q);
      pay(u, q);
      counts.deposit = (counts.deposit ?? 0) + 1;
    } else if (roll < 0.7 && active.some((u) => held.get(u)! + min <= cap)) {
      const u = pick(active.filter((x) => held.get(x)! + min <= cap));
      const q = between(min, cap - held.get(u)!);
      const m = await program.account.member.fetch(u.member);
      await send("top_up", topUpIx(u, q, siblings(await mirror(), m.leafIndex)), [u.wallet]);
      held.set(u, held.get(u)! + q);
      pay(u, q);
      counts.top_up = (counts.top_up ?? 0) + 1;
    } else if (roll < 0.8 && active.length) {
      const u = pick(active);
      const m = await program.account.member.fetch(u.member);
      await send("slash", program.methods.slash(Array.from(toBytes32(u.secret)), siblings(await mirror(), m.leafIndex))
        .accountsPartial({ reporter: authority.publicKey, member: u.member }));
      slashed.add(u);
      counts.slash = (counts.slash ?? 0) + 1;
    } else {
      await send("sweep_treasury", program.methods.sweepTreasury().accountsPartial({ treasury }));
      counts.sweep = (counts.sweep ?? 0) + 1;
    }

    const c = await config();
    assert.equal(await balance(A.vault), base.vault, `step ${step}: nothing may be held for members`);
    assert.equal((await balance(A.treasuryVault)) + (await balance(treasury)) - base.treasury, paidTreasury, `step ${step}: treasury`);
    assert.equal((await balance(A.burnVault)) - base.burn, paidBurn, `step ${step}: burn vault`);
    assert.equal(BigInt(c.deposited.toString()) - base.deposited, paidTreasury + paidBurn, `step ${step}: deposited`);
    for (const u of users) {
      assert.equal(await balance(u.usdc), START - spent.get(u)!, `step ${step}: a wallet's USDC`);
      if (!held.has(u)) continue;
      const m = await program.account.member.fetch(u.member);
      assert.equal(Number(m.questions.toString()), held.get(u), `step ${step}: ticket count`);
      assert.ok(Number(m.questions.toString()) <= cap, `step ${step}: cap`);
      assert.deepEqual(Object.keys(m.status), [slashed.has(u) ? "slashed" : "active"]);
    }
    assert.equal((await mirror()).root(), currentRoot(await treeState()), `step ${step}: SDK mirror and chain root`);
  }
  console.log(`fuzz seed ${SEED}: ${JSON.stringify(counts)}, ${RATE} per question, cap ${PARAMS.limit}`);
});
