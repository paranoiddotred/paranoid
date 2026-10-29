// Reading the PARANOID program from a client or a gateway: addresses, the root history, the
// member set, and the Merkle siblings the top-up and slash instructions need.
import { PublicKey } from "@solana/web3.js";
import { DEPTH, MemberTree, fromBytes, leafOf, toBytes32, type Hash } from "./rln.ts";

export const PROGRAM_ID = new PublicKey("9JJqzEoDGkTE8CMLATRw7CqFhcyWVegfhFuEeGfBfVGm");
export const USDC_MINT = new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
export const ROOT_HISTORY = 64;

const pda = (seeds: (Buffer | Uint8Array)[], programId: PublicKey) => PublicKey.findProgramAddressSync(seeds, programId)[0];

export function addresses(programId = PROGRAM_ID) {
  return {
    config: pda([Buffer.from("config")], programId),
    tree: pda([Buffer.from("tree")], programId),
    vault: pda([Buffer.from("vault")], programId),
    treasuryVault: pda([Buffer.from("treasury_vault")], programId),
    burnVault: pda([Buffer.from("burn_vault")], programId),
    bought: pda([Buffer.from("bought")], programId),
    wsolVault: pda([Buffer.from("wsol")], programId),
    burnRoute: pda([Buffer.from("burn_route")], programId),
    member: (commitment: bigint) => pda([Buffer.from("member"), toBytes32(commitment)], programId),
  };
}

export interface RootEntry {
  root: bigint;
  /** Accepted for epochs strictly below this; 2^32 - 1 means open. */
  validBefore: number;
}

export interface TreeState {
  nextIndex: number;
  head: number;
  roots: RootEntry[];
}

/** Zero-copy layout of `Tree`: discriminator, next_index, head, zeros[21], filled[20], roots[64]. */
export function decodeTree(data: Buffer): TreeState {
  const nextIndex = data.readUInt32LE(8);
  const head = data.readUInt32LE(12);
  const rootsAt = 16 + 32 * (DEPTH + 1) + 32 * DEPTH;
  const roots: RootEntry[] = [];
  for (let i = 0; i < ROOT_HISTORY; i++) {
    const at = rootsAt + i * 36;
    roots.push({ root: fromBytes(data.subarray(at, at + 32)), validBefore: data.readUInt32LE(at + 32) });
  }
  return { nextIndex, head, roots };
}

export function currentRoot(tree: TreeState): bigint {
  return tree.roots[tree.head].root;
}

/** The roots a gateway accepts for a request made in `epoch`. */
export function acceptedRoots(tree: TreeState, epoch: number): Set<bigint> {
  const out = new Set<bigint>();
  for (const r of tree.roots) if (r.root !== 0n && r.validBefore > epoch) out.add(r.root);
  return out;
}

export function epochAt(config: { genesis: { toNumber(): number } | number; epochSeconds: number }, unixSeconds: number): number {
  const genesis = typeof config.genesis === "number" ? config.genesis : config.genesis.toNumber();
  return Math.floor(Math.max(0, unixSeconds - genesis) / config.epochSeconds);
}

export type Status = "active" | "slashed";

export interface MemberView {
  address: PublicKey;
  depositor: PublicKey;
  commitment: bigint;
  leafIndex: number;
  startEpoch: number;
  endEpoch: number;
  /** Tickets bought, numbered 0 .. questions - 1. */
  questions: number;
  status: Status;
}

/** Normalises an Anchor-decoded `Member` account. */
export function memberView(address: PublicKey, a: any): MemberView {
  return {
    address,
    depositor: a.depositor,
    commitment: fromBytes(a.commitment),
    leafIndex: a.leafIndex,
    startEpoch: a.startEpoch,
    endEpoch: a.endEpoch,
    questions: Number(a.questions.toString()),
    status: Object.keys(a.status)[0] as Status,
  };
}

/** Rebuilds the on-chain tree: active members hold their leaf, every other index is empty. */
export function buildTree(hash: Hash, members: MemberView[]): MemberTree {
  const tree = new MemberTree(hash);
  for (const m of members) if (m.status === "active") tree.set(m.leafIndex, leafOf(hash, m.commitment, m.endEpoch, m.questions));
  return tree;
}

/**
 * Members a request made on day `epoch` hides among: everyone whose tickets are still valid that day.
 * A client should not send when this is small; the app shows it next to the send button.
 */
export function anonymitySet(members: MemberView[], epoch: number): number {
  return members.filter((m) => m.status === "active" && m.startEpoch <= epoch && epoch <= m.endEpoch).length;
}

/** Sibling list for an instruction argument (`siblings: Vec<[u8; 32]>`). */
export function siblings(tree: MemberTree, index: number): number[][] {
  return tree.path(index).pathElements.map((e) => Array.from(toBytes32(e)));
}
