// Gift card order escrow: the buyer pays into a program vault for a throwaway key; only that key can
// send the money on, and the buyer can always take it back to their own account. Nothing else moves it.
import { test } from "node:test";
import assert from "node:assert/strict";
import { Keypair, PublicKey } from "@solana/web3.js";
import { getAccount } from "@solana/spl-token";
import { BN, airdrop, balance, connection, ensureInitialized, expectError, newUser, program, send, usdcAccount } from "./harness.ts";
import { PROGRAM_ID } from "../sdk/chain.ts";

const pda = (seed: string, key: PublicKey) => PublicKey.findProgramAddressSync([Buffer.from(seed), key.toBuffer()], PROGRAM_ID)[0];
const order = (key: PublicKey) => pda("order", key);
const vault = (key: PublicKey) => pda("order_vault", key);

const open = (buyer: Keypair, source: PublicKey, key: PublicKey, amount: bigint) =>
  program.methods.openOrder(new BN(amount.toString())).accountsPartial({ buyer: buyer.publicKey, key, order: order(key), vault: vault(key), source }).signers([buyer]);
const settle = (signer: Keypair, key: PublicKey, buyer: PublicKey, destination: PublicKey) =>
  program.methods.settleOrder().accountsPartial({ signer: signer.publicKey, order: order(key), buyer, vault: vault(key), destination }).signers([signer]);

await ensureInitialized(await usdcAccount(Keypair.generate().publicKey));

test("open_order: the buyer's USDC goes into the order's own vault, in one instruction", async () => {
  const u = await newUser(20_000_000n);
  const key = Keypair.generate().publicKey;
  const sig = await send("open_order", open(u.wallet, u.usdc, key, 5_250_000n));
  assert.equal(await balance(vault(key)), 5_250_000n);
  assert.equal(await balance(u.usdc), 14_750_000n);
  const o = await program.account.order.fetch(order(key));
  assert.ok(o.buyer.equals(u.wallet.publicKey) && o.key.equals(key) && o.amount.toString() === "5250000");
  const tx = await connection.getTransaction(sig, { commitment: "confirmed", maxSupportedTransactionVersion: 0 });
  assert.equal(tx!.transaction.message.compiledInstructions.length, 1);
});

test("open_order: nothing, too much, or the same key twice is refused", async () => {
  const u = await newUser(2_000_000_000n);
  const key = Keypair.generate().publicKey;
  await expectError(open(u.wallet, u.usdc, key, 0n).rpc(), "OrderAmountInvalid");
  await expectError(open(u.wallet, u.usdc, key, 1_000_000_001n).rpc(), "OrderAmountInvalid");
  await send("open_order", open(u.wallet, u.usdc, key, 1_000_000n));
  await expectError(open(u.wallet, u.usdc, key, 1_000_000n).rpc(), "already in use");
});

test("settle_order by the key: all of it to the key's account, both accounts closed, deposits back to the buyer", async () => {
  const u = await newUser(20_000_000n);
  const k = Keypair.generate();
  await airdrop(k.publicKey, 1);
  const keyUsdc = await usdcAccount(k.publicKey);
  await send("open_order", open(u.wallet, u.usdc, k.publicKey, 5_250_000n));
  const lamportsBefore = await connection.getBalance(u.wallet.publicKey);
  await send("settle_order", settle(k, k.publicKey, u.wallet.publicKey, keyUsdc));
  assert.equal(await balance(keyUsdc), 5_250_000n);
  assert.equal(await connection.getAccountInfo(order(k.publicKey)), null);
  assert.equal(await connection.getAccountInfo(vault(k.publicKey)), null);
  assert.ok((await connection.getBalance(u.wallet.publicKey)) > lamportsBefore, "the buyer got the account deposits back");
});

test("settle_order by the buyer: only back to the buyer's own account", async () => {
  const u = await newUser(20_000_000n);
  const key = Keypair.generate().publicKey;
  const other = await usdcAccount(Keypair.generate().publicKey);
  await send("open_order", open(u.wallet, u.usdc, key, 3_000_000n));
  await expectError(settle(u.wallet, key, u.wallet.publicKey, other).rpc(), "NotOrderParty");
  await send("settle_order", settle(u.wallet, key, u.wallet.publicKey, u.usdc));
  assert.equal(await balance(u.usdc), 20_000_000n);
  assert.equal(await connection.getAccountInfo(order(key)), null);
});

test("settle_order: a stranger cannot take it, and the deposits cannot be sent to anyone but the buyer", async () => {
  const u = await newUser(20_000_000n);
  const key = Keypair.generate().publicKey;
  const thief = Keypair.generate();
  await airdrop(thief.publicKey, 1);
  const thiefUsdc = await usdcAccount(thief.publicKey);
  await send("open_order", open(u.wallet, u.usdc, key, 3_000_000n));
  await expectError(settle(thief, key, u.wallet.publicKey, thiefUsdc).rpc(), "NotOrderParty");
  await expectError(settle(u.wallet, key, thief.publicKey, u.usdc).rpc(), "ConstraintHasOne");
  assert.equal((await getAccount(connection, vault(key))).amount, 3_000_000n);
});
