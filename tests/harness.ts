// Shared setup for the suites that run against our own solana-test-validator
// (scripts/localnet.sh up): connection, program client, test USDC, clock and epoch helpers,
// error matching and compute-unit capture.
import anchor from "@coral-xyz/anchor";
import {
  Connection, Keypair, LAMPORTS_PER_SOL, PublicKey, SYSVAR_CLOCK_PUBKEY, type TransactionSignature,
} from "@solana/web3.js";
import { createAssociatedTokenAccount, getAccount, mintTo } from "@solana/spl-token";
import { readFileSync } from "node:fs";
import { addresses, buildTree, decodeTree, epochAt, memberView, PROGRAM_ID, USDC_MINT, type MemberView } from "../sdk/chain.ts";
import { commitmentOf, loadHash, randomSecret, toBytes32, type Hash } from "../sdk/rln.ts";

export const RPC = `http://127.0.0.1:${process.env.PARANOID_RPC_PORT ?? 18929}`;
export const connection = new Connection(RPC, "confirmed");
const key = (path: string) => Keypair.fromSecretKey(Uint8Array.from(JSON.parse(readFileSync(path, "utf8"))));
export const authority = key("fixtures/test-authority.json");
export const usdcAuthority = key("fixtures/usdc-authority.json");
export const idl = JSON.parse(readFileSync("target/idl/paranoid.json", "utf8"));
export const provider = new anchor.AnchorProvider(connection, new anchor.Wallet(authority), { commitment: "confirmed" });
export const program = new anchor.Program(idl, provider);
export const A = addresses(PROGRAM_ID);
export const BN = anchor.BN;
export const hash: Hash = await loadHash();

/** Launch constants the local suites run with (the localnet build allows short days). */
export const PARAMS = {
  epochSeconds: 8,
  rate: new BN(2_000),
  limit: 1_000,
  maxEpochs: 30,
  minQuestions: 250,
  burnBps: 2_000,
  slashBurnBps: 5_000,
};
export const RATE = 2_000n;

export async function airdrop(to: PublicKey, sol = 10) {
  const sig = await connection.requestAirdrop(to, sol * LAMPORTS_PER_SOL);
  await connection.confirmTransaction(sig, "confirmed");
}

if ((await connection.getBalance(authority.publicKey)) < 50 * LAMPORTS_PER_SOL) await airdrop(authority.publicKey, 100);

export async function usdcAccount(owner: PublicKey, amount = 0n): Promise<PublicKey> {
  const ata = await createAssociatedTokenAccount(connection, authority, USDC_MINT, owner);
  if (amount > 0n) await mintTo(connection, authority, USDC_MINT, ata, usdcAuthority, amount);
  return ata;
}

export async function balance(account: PublicKey): Promise<bigint> {
  return (await getAccount(connection, account)).amount;
}

export async function chainTime(): Promise<number> {
  const info = await connection.getAccountInfo(SYSVAR_CLOCK_PUBKEY);
  return Number(info!.data.readBigInt64LE(32));
}

export async function config(): Promise<any> {
  return program.account.config.fetch(A.config);
}

export async function epochNow(): Promise<number> {
  return epochAt(await config(), await chainTime());
}

export async function waitForEpoch(epoch: number): Promise<void> {
  while ((await epochNow()) < epoch) await new Promise((r) => setTimeout(r, 400));
}

/** Waits until at least `margin` seconds remain in the current epoch, so a step can't straddle two. */
export async function freshEpoch(margin = 3): Promise<number> {
  const c = await config();
  if (margin >= c.epochSeconds) throw new Error("margin must be shorter than an epoch");
  for (;;) {
    const t = await chainTime();
    const into = (t - c.genesis.toNumber()) % c.epochSeconds;
    if (c.epochSeconds - into > margin) return epochAt(c, t);
    await new Promise((r) => setTimeout(r, 400));
  }
}

export async function treeState() {
  const info = await connection.getAccountInfo(A.tree);
  return decodeTree(info!.data);
}

export async function members(): Promise<MemberView[]> {
  const all = await program.account.member.all();
  return all.map((m: any) => memberView(m.publicKey, m.account));
}

export async function mirror() {
  return buildTree(hash, await members());
}

export const computeUnits = new Map<string, number[]>();

/** Sends a builder's transaction and records its compute units under `label`. */
export async function send(label: string, builder: any, signers: Keypair[] = []): Promise<TransactionSignature> {
  const sig: TransactionSignature = await builder.signers(signers).rpc();
  const tx = await connection.getTransaction(sig, { commitment: "confirmed", maxSupportedTransactionVersion: 0 });
  const used = tx?.meta?.computeUnitsConsumed;
  if (used !== undefined) computeUnits.set(label, [...(computeUnits.get(label) ?? []), Number(used)]);
  return sig;
}

/** Resolves when `promise` fails with the program error `code` (or a message containing it). */
export async function expectError(promise: Promise<unknown>, code: string): Promise<void> {
  try {
    await promise;
  } catch (e: any) {
    const got = e?.error?.errorCode?.code ?? "";
    const text = `${got} ${e?.message ?? ""} ${(e?.logs ?? e?.transactionLogs ?? []).join("\n")}`;
    if (got === code || text.includes(code)) return;
    throw new Error(`expected ${code}, got: ${text.slice(0, 600)}`);
  }
  throw new Error(`expected ${code}, but the transaction succeeded`);
}


export interface User {
  wallet: Keypair;
  usdc: PublicKey;
  secret: bigint;
  commitment: bigint;
  member: PublicKey;
}

export async function newUser(usdc = 100_000_000n): Promise<User> {
  const wallet = Keypair.generate();
  await airdrop(wallet.publicKey);
  const secret = randomSecret();
  const commitment = commitmentOf(hash, secret);
  return { wallet, usdc: await usdcAccount(wallet.publicKey, usdc), secret, commitment, member: A.member(commitment) };
}

export function depositIx(u: User, questions: number) {
  return program.methods
    .deposit(Array.from(toBytes32(u.commitment)), questions)
    .accountsPartial({ depositor: u.wallet.publicKey, member: u.member, source: u.usdc });
}

export function topUpIx(u: User, questions: number, path: number[][]) {
  return program.methods
    .topUp(questions, path)
    .accountsPartial({ depositor: u.wallet.publicKey, member: u.member, source: u.usdc });
}

export async function ensureInitialized(treasury: PublicKey): Promise<void> {
  if (await connection.getAccountInfo(A.config)) return;
  const programData = PublicKey.findProgramAddressSync([PROGRAM_ID.toBuffer()], new PublicKey("BPFLoaderUpgradeab1e11111111111111111111111"))[0];
  await send("initialize", program.methods.initialize(PARAMS).accountsPartial({ authority: authority.publicKey, programData, treasury }));
}

export function report(): string {
  const rows = [...computeUnits.entries()].map(([k, v]) => `${k}: max ${Math.max(...v)} CU, min ${Math.min(...v)} CU (${v.length} txs)`);
  return rows.join("\n");
}
