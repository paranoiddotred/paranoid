// Staking $PARANOID for a share of the buybacks, on our own validator holding the real mainnet PARANOID
// mint, one holder account and the real Meteora pool (fixtures/accounts). A stake belongs to a wallet;
// every purchase buys $PARANOID and half of each buy goes to the stakes that are not unstaking.
// Local days last 8 s, so the 3-day unstake wait is 24 s.
import { before, test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { Keypair, PublicKey, SystemProgram, Transaction } from "@solana/web3.js";
import {
  TOKEN_PROGRAM_ID, createAssociatedTokenAccount, createAssociatedTokenAccountInstruction, createSyncNativeInstruction,
  getAssociatedTokenAddressSync, getMint, transfer,
} from "@solana/spl-token";
import { PROGRAM_ID } from "../sdk/chain.ts";
import { A, BN, airdrop, authority, balance, chainTime, config, connection, depositIx, ensureInitialized, expectError, newUser, program, send, usdcAccount, RATE } from "./harness.ts";

const MINT = new PublicKey("22rWiyDiY9GgxSrFiWC2vX1GxNxjkvrRPmZJDzv9pq6X");
const TOKEN = 1_000_000_000n; // 9 decimals
const SCALE = 1_000_000_000_000n;
const holder = Keypair.fromSecretKey(Uint8Array.from(JSON.parse(readFileSync("fixtures/token-holder.json", "utf8"))));
const holderAta = getAssociatedTokenAddressSync(MINT, holder.publicKey);
const pda = (...seeds: (string | Buffer)[]) => PublicKey.findProgramAddressSync(seeds.map((s) => (typeof s === "string" ? Buffer.from(s) : s)), PROGRAM_ID)[0];
const stake = pda("stake"), vault = pda("stake_vault"), reward = pda("stake_reward"), rewardVault = pda("stake_reward_vault");
const position = (owner: PublicKey) => pda("wallet_stake", owner.toBuffer());
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

type Staker = { wallet: Keypair; tokens: PublicKey };
async function staker(tokens: bigint): Promise<Staker> {
  const wallet = Keypair.generate();
  await airdrop(wallet.publicKey);
  const ata = await createAssociatedTokenAccount(connection, authority, MINT, wallet.publicKey);
  if (tokens > 0n) await transfer(connection, authority, holderAta, ata, holder, tokens);
  return { wallet, tokens: ata };
}
const start = (u: Staker, amount: bigint) =>
  program.methods.startStake(new BN(amount.toString())).accountsPartial({ owner: u.wallet.publicKey, source: u.tokens }).signers([u.wallet]);
const add = (u: Staker, amount: bigint) =>
  program.methods.addStake(new BN(amount.toString())).accountsPartial({ owner: u.wallet.publicKey, source: u.tokens }).signers([u.wallet]);
const unstake = (u: Staker) => program.methods.requestUnstake().accountsPartial({ owner: u.wallet.publicKey }).signers([u.wallet]);
const withdraw = (u: Staker, destination = u.tokens, signer = u.wallet) =>
  program.methods.withdrawStake().accountsPartial({ owner: signer.publicKey, position: position(u.wallet.publicKey), destination }).signers([signer]);
const claim = (u: Staker, destination = u.tokens, signer = u.wallet) =>
  program.methods.claimStakeReward().accountsPartial({ owner: signer.publicKey, position: position(u.wallet.publicKey), destination }).signers([signer]);
/** Waits until the stake's 3-day (epoch) unstake wait is over on the chain clock. */
async function waitUnstake(u: Staker) {
  const at = Number((await program.account.walletStake.fetch(position(u.wallet.publicKey))).unstakeAt.toString());
  while ((await chainTime()) < at) await sleep(2_000);
}

before(async () => {
  await ensureInitialized(await usdcAccount(authority.publicKey));
  await airdrop(holder.publicKey);
});

const openStake = () => program.methods.openStake().accountsPartial({ payer: authority.publicKey });
const openReward = () => program.methods.openStakeReward().accountsPartial({ payer: authority.publicKey });

test("open_stake and open_stake_reward: anyone, once", async () => {
  // tests/program.test.ts opens both before its burn swap; alone, this file opens them here
  if (!(await connection.getAccountInfo(stake))) await send("open_stake", openStake());
  if (!(await connection.getAccountInfo(reward))) await send("open_stake_reward", openReward());
  assert.equal((await program.account.stake.fetch(stake)).staked.toString(), "0");
  assert.equal((await program.account.stakeReward.fetch(reward)).earning.toString(), "0");
  await assert.rejects(openStake().rpc(), "a second call finds the stake in use");
  await assert.rejects(openReward().rpc(), "a second call finds the share in use");
});

test("wallet stake: start, add, unstake after a 3-day wait; only the owner moves it; every token comes back", async () => {
  const u = await staker(1_500_000n * TOKEN);
  const stranger = await staker(0n);
  await expectError(start(u, 0n).rpc(), "StakeTooSmall");
  await send("start_stake", start(u, 1_000_000n * TOKEN));
  assert.equal(await balance(vault), 1_000_000n * TOKEN, "the vault holds the stake");
  await assert.rejects(start(u, 1n).rpc(), "one stake per wallet");
  await send("add_stake", add(u, 500_000n * TOKEN));
  assert.equal(await balance(u.tokens), 0n);
  const p = await program.account.walletStake.fetch(position(u.wallet.publicKey));
  assert.equal(p.owner.toBase58(), u.wallet.publicKey.toBase58());
  assert.equal(p.amount.toString(), (1_500_000n * TOKEN).toString());
  assert.equal((await program.account.stakeReward.fetch(reward)).earning.toString(), (1_500_000n * TOKEN).toString());
  await expectError(claim(u).rpc(), "NothingToClaim");

  await expectError(withdraw(u).rpc(), "UnstakeNotReady");
  await expectError(withdraw(u, stranger.tokens, stranger.wallet).rpc(), "ConstraintSeeds");
  await send("request_unstake", unstake(u));
  const pu = await program.account.walletStake.fetch(position(u.wallet.publicKey));
  const epochSeconds = Number((await config()).epochSeconds);
  assert.ok(Math.abs(Number(pu.unstakeAt.toString()) - ((await chainTime()) + 3 * epochSeconds)) <= 3, "the wait is 3 days");
  assert.equal((await program.account.stakeReward.fetch(reward)).earning.toString(), "0", "an unstaking stake stops sharing");
  await expectError(withdraw(u).rpc(), "UnstakeNotReady");
  await expectError(unstake(u).rpc(), "Unstaking");
  await expectError(add(u, 1n).rpc(), "Unstaking");
  await waitUnstake(u);
  await expectError(withdraw(u, stranger.tokens).rpc(), "ConstraintTokenOwner");
  await send("withdraw_stake", withdraw(u));
  assert.equal(await balance(u.tokens), 1_500_000n * TOKEN, "every staked token comes back");
  assert.equal(await connection.getAccountInfo(position(u.wallet.publicKey)), null, "the stake is closed");
  assert.equal(await balance(vault), 0n);
  const s = await program.account.stake.fetch(stake);
  assert.equal(s.staked.toString(), "0");
  assert.equal(s.positions.toString(), "0");
});

test("buyback: every purchase buys $PARANOID; stakers share half by size and claim it; the rest burns", async () => {
  const DAMM_POOL = new PublicKey("5Diq3hfJ6QTcJXhAG1XVBiSLUQJHNue24YkkzLKmJooH");
  const PYTH = new PublicKey("7UVimffxr9ow1uXYxsr4LHAcV58mLzhmwaeKvJ1pjLiE");
  const WSOL = new PublicKey("So11111111111111111111111111111111111111112");
  const poolData = (await connection.getAccountInfo(DAMM_POOL))!.data;
  const poolVaultA = new PublicKey(poolData.subarray(232, 264));
  const poolVaultB = new PublicKey(poolData.subarray(264, 296));

  const a = await staker(1_000_000n * TOKEN), b = await staker(3_000_000n * TOKEN), c = await staker(1_000_000n * TOKEN);
  await send("start_stake a", start(a, 1_000_000n * TOKEN));
  await send("start_stake b", start(b, 3_000_000n * TOKEN));
  await send("start_stake c", start(c, 1_000_000n * TOKEN));
  await send("request_unstake c", unstake(c));
  assert.equal((await program.account.stakeReward.fetch(reward)).earning.toString(), (4_000_000n * TOKEN).toString(), "an unstaking stake does not share");

  // a purchase sends all its USDC to the burn vault; a keeper turns it into wSOL at the oracle price
  const buyer = await newUser(1_000n * RATE);
  const usdc0 = await balance(A.burnVault);
  await send("deposit (1,000 questions)", depositIx(buyer, 1_000), [buyer.wallet]);
  assert.equal((await balance(A.burnVault)) - usdc0, 1_000n * RATE, "the whole purchase goes to the buyback");
  const keeper = Keypair.generate();
  await airdrop(keeper.publicKey);
  const keeperWsol = getAssociatedTokenAddressSync(WSOL, keeper.publicKey);
  const keeperUsdc = await usdcAccount(keeper.publicKey);
  await connection.sendTransaction(new Transaction().add(
    createAssociatedTokenAccountInstruction(keeper.publicKey, keeperWsol, keeper.publicKey, WSOL),
    SystemProgram.transfer({ fromPubkey: keeper.publicKey, toPubkey: keeperWsol, lamports: 2_000_000_000 }),
    createSyncNativeInstruction(keeperWsol),
  ), [keeper]).then((sig) => connection.confirmTransaction(sig, "confirmed"));
  const usdc = await balance(A.burnVault);
  await send("convert_burn", program.methods.convertBurn(new BN((usdc < 25_000_000n ? usdc : 25_000_000n).toString()))
    .accountsPartial({ caller: keeper.publicKey, callerUsdc: keeperUsdc, callerWsol: keeperWsol, priceUpdate: PYTH }).signers([keeper]));

  const route = await program.account.burnRoute.fetch(pda("burn_route"));
  while ((await connection.getSlot("confirmed")) < Number(route.lastSwapSlot.toString()) + 151) await sleep(1_000);
  const supply0 = (await getMint(connection, MINT)).supply;
  const burned0 = BigInt((await config()).tokenBurned.toString());
  await send("crank_burn (half to stakers)", program.methods.crankBurn().accountsPartial({
    caller: keeper.publicKey, burnMint: MINT, wsolMint: WSOL, pool: DAMM_POOL, poolVaultA, poolVaultB,
    poolAuthority: new PublicKey("HLnpSz9h2S4hiLQ43rnSD9XkcUThA7B8hQMKmDaiTLcC"),
    eventAuthority: new PublicKey("3rmHSu74h1ZcmAisVcWerTCiRDQbUrBKmcwptYGjHfet"),
    dammProgram: new PublicKey("cpamdpZCGKUy5JxQXB4dcpGPiikHawvSWAd6mEn1sGG"),
    burnTokenProgram: TOKEN_PROGRAM_ID,
  }).signers([keeper]));
  const r1 = await program.account.stakeReward.fetch(reward);
  const given = BigInt(r1.given.toString());
  const burned = BigInt((await config()).tokenBurned.toString()) - burned0;
  console.log(`crank bought ${Number(given + burned) / 1e9} $PARANOID: ${Number(given) / 1e9} to stakers, ${Number(burned) / 1e9} burned`);
  assert.ok(given > 0n, "the crank bought tokens");
  assert.equal(given, (given + burned) / 2n, "half of the buy goes to stakers (rounded down)");
  assert.equal(await balance(rewardVault), given, "the stakers' half waits for them");
  assert.equal((await getMint(connection, MINT)).supply, supply0 - burned, "the other half is burned");
  const perToken = BigInt(r1.perToken.toString());
  assert.equal(perToken, (given * SCALE) / (4_000_000n * TOKEN));

  // each stake's share follows its size; the unstaking one gets nothing; only the owner claims
  await expectError(claim(a, b.tokens, b.wallet).rpc(), "ConstraintSeeds");
  await send("claim a", claim(a));
  const forA = (1_000_000n * TOKEN * perToken) / SCALE;
  assert.equal(await balance(a.tokens), forA, "a quarter of the stakers' half");
  await expectError(claim(a).rpc(), "NothingToClaim");
  await expectError(claim(c).rpc(), "NothingToClaim");
  assert.equal((await program.account.walletStake.fetch(position(a.wallet.publicKey))).amount.toString(), (1_000_000n * TOKEN).toString(), "claiming leaves the stake");

  // unstaking takes the stake back with the unclaimed share
  await send("request_unstake b", unstake(b));
  await send("request_unstake a", unstake(a));
  await waitUnstake(b);
  await waitUnstake(a);
  await waitUnstake(c);
  const forB = (3_000_000n * TOKEN * perToken) / SCALE;
  await send("withdraw_stake b", withdraw(b));
  assert.equal(await balance(b.tokens), 3_000_000n * TOKEN + forB, "the stake and three quarters of the stakers' half");
  await send("withdraw_stake a", withdraw(a));
  await send("withdraw_stake c", withdraw(c));
  assert.equal(await balance(c.tokens), 1_000_000n * TOKEN, "the unstaking stake got back exactly what it staked");
  const r2 = await program.account.stakeReward.fetch(reward);
  assert.equal(BigInt(r2.claimed.toString()), forA + forB);
  assert.equal(await balance(rewardVault), given - forA - forB, "only rounding dust stays");
  // per_token rounds down by under one unit of 1/SCALE per staked base unit: under a millionth of a token here
  assert.ok(given - forA - forB <= (4_000_000n * TOKEN) / SCALE + 2n, String(given - forA - forB));
  assert.equal(r2.earning.toString(), "0");
  assert.equal(await balance(vault), 0n);
});
