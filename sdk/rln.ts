// Client and gateway side of a PARANOID request: identities, the member tree mirror, proving,
// verifying, and the nullifier book that turns a double use into a recovered secret.
// Runs in Node 22+ and in the browser (crypto.getRandomValues, crypto.subtle).
import { buildPoseidon } from "circomlibjs";
import * as snarkjs from "snarkjs";

export const FIELD = 21888242871839275222246405745257275088548364400416034343698204186575808495617n;
/** Must match `Request(20, 4)` in circuits/request.circom and the program's TREE_DEPTH / MAX_WEIGHT. */
export const DEPTH = 20;
export const SLOTS = 4;
/** Domain of the external nullifier: "paranoid.v2" as a field element. One namespace for a member's life. */
export const APP = 0x706172616e6f69642e7632n;

export type Hash = (inputs: bigint[]) => bigint;

export async function loadHash(): Promise<Hash> {
  const poseidon = await buildPoseidon();
  return (inputs) => poseidon.F.toObject(poseidon(inputs)) as bigint;
}

const mod = (a: bigint): bigint => ((a % FIELD) + FIELD) % FIELD;

function inverse(a: bigint): bigint {
  let [r0, r1, s0, s1] = [mod(a), FIELD, 1n, 0n];
  if (r0 === 0n) throw new Error("no inverse of zero");
  while (r1 !== 0n) {
    const q = r0 / r1;
    [r0, r1] = [r1, r0 - q * r1];
    [s0, s1] = [s1, s0 - q * s1];
  }
  return mod(s0);
}

export function toBytes32(value: bigint): Uint8Array {
  return Uint8Array.from(Buffer.from(value.toString(16).padStart(64, "0"), "hex"));
}

export function fromBytes(bytes: Uint8Array | number[]): bigint {
  return BigInt("0x" + Buffer.from(bytes).toString("hex"));
}

/** A fresh identity secret: 31 random bytes, so it is always below the field modulus. */
export function randomSecret(): bigint {
  const bytes = new Uint8Array(31);
  crypto.getRandomValues(bytes);
  const secret = fromBytes(bytes);
  return secret === 0n ? randomSecret() : secret;
}

export function commitmentOf(hash: Hash, secret: bigint): bigint {
  return hash([secret]);
}

/** leaf = Poseidon(commitment, endEpoch, questions): the member, the last day it is valid, the tickets it bought. */
export function leafOf(hash: Hash, commitment: bigint, endEpoch: number, questions: number): bigint {
  return hash([commitment, BigInt(endEpoch), BigInt(questions)]);
}

/** The value `x` a proof is bound to: SHA-256 of the exact request bytes, cut to 248 bits. */
export async function messageHash(body: Uint8Array): Promise<bigint> {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", body));
  const x = fromBytes(digest.subarray(0, 31));
  // x = 0 would publish y = secret; the chance is 2^-248 but the check is free.
  if (x === 0n) throw new Error("message hash is zero");
  return x;
}

/** Sparse mirror of the on-chain member tree: leaf i is a member's leaf or 0 once it left. */
export class MemberTree {
  readonly zeros: bigint[];
  readonly depth: number;
  private readonly hash: Hash;
  private readonly levels: Map<number, bigint>[];

  constructor(hash: Hash, depth = DEPTH) {
    this.hash = hash;
    this.depth = depth;
    this.zeros = [0n];
    for (let i = 0; i < depth; i++) this.zeros.push(hash([this.zeros[i], this.zeros[i]]));
    this.levels = Array.from({ length: depth + 1 }, () => new Map<number, bigint>());
  }

  set(index: number, leaf: bigint): void {
    let position = index;
    let node = leaf;
    for (let i = 0; i <= this.depth; i++) {
      if (node === this.zeros[i]) this.levels[i].delete(position);
      else this.levels[i].set(position, node);
      if (i === this.depth) break;
      const left = position % 2 === 0 ? node : this.node(i, position - 1);
      const right = position % 2 === 0 ? this.node(i, position + 1) : node;
      node = this.hash([left, right]);
      position = Math.floor(position / 2);
    }
  }

  node(level: number, position: number): bigint {
    return this.levels[level].get(position) ?? this.zeros[level];
  }

  root(): bigint {
    return this.node(this.depth, 0);
  }

  path(index: number): { pathElements: bigint[]; pathIndices: number[] } {
    const pathElements: bigint[] = [];
    const pathIndices: number[] = [];
    let position = index;
    for (let i = 0; i < this.depth; i++) {
      pathElements.push(this.node(i, position ^ 1));
      pathIndices.push(position & 1);
      position = Math.floor(position / 2);
    }
    return { pathElements, pathIndices };
  }
}

export interface Artifacts {
  /** Witness generator: build/request_js/request.wasm (path in Node, URL or bytes in a browser). */
  wasm: string | Uint8Array;
  /** Proving key: build/request.zkey. */
  zkey: string | Uint8Array;
}

export interface RequestWitness {
  secret: bigint;
  endEpoch: number;
  /** Tickets this member bought (bound into its leaf). */
  questions: number;
  leafIndex: number;
  tree: MemberTree;
  epoch: number;
  /** First ticket this request spends; tickets never come back, so the client keeps one counter for life. */
  start: number;
  /** Tickets spent, 1..SLOTS. */
  weight: number;
  x: bigint;
  limit: number;
}

export interface PublicRequest {
  nullifiers: bigint[];
  ys: bigint[];
  root: bigint;
  epoch: number;
  x: bigint;
  limit: number;
  weight: number;
}

/** snarkjs Groth16 proof object (pi_a, pi_b, pi_c, protocol, curve). */
export type Groth16Proof = Record<string, unknown>;

export interface Proven {
  proof: Groth16Proof;
  publicSignals: string[];
}

export async function proveRequest(w: RequestWitness, artifacts: Artifacts): Promise<Proven> {
  const { pathElements, pathIndices } = w.tree.path(w.leafIndex);
  const input = {
    root: w.tree.root().toString(),
    epoch: w.epoch,
    x: w.x.toString(),
    limit: w.limit,
    weight: w.weight,
    secret: w.secret.toString(),
    endEpoch: w.endEpoch,
    questions: w.questions,
    start: w.start,
    pathElements: pathElements.map(String),
    pathIndices,
  };
  const { proof, publicSignals } = await snarkjs.groth16.fullProve(input, artifacts.wasm, artifacts.zkey);
  return { proof, publicSignals };
}

/** snarkjs order: outputs (nullifiers[SLOTS], ys[SLOTS]) then public inputs (root, epoch, x, limit, weight). */
export function readPublic(publicSignals: string[]): PublicRequest {
  if (publicSignals.length !== 2 * SLOTS + 5) throw new Error("wrong number of public signals");
  const v = publicSignals.map((s) => BigInt(s));
  return {
    nullifiers: v.slice(0, SLOTS),
    ys: v.slice(SLOTS, 2 * SLOTS),
    root: v[2 * SLOTS],
    epoch: Number(v[2 * SLOTS + 1]),
    x: v[2 * SLOTS + 2],
    limit: Number(v[2 * SLOTS + 3]),
    weight: Number(v[2 * SLOTS + 4]),
  };
}

export interface GatewayRules {
  /** Roots the chain accepts for `epoch` (see chain.ts acceptedRoots). */
  acceptedRoots: Set<bigint>;
  epoch: number;
  limit: number;
  /** messageHash of the exact body the gateway is about to forward. */
  x: bigint;
}

export type Verdict = { ok: true; request: PublicRequest } | { ok: false; reason: string };

/** Everything a gateway checks before it looks at the nullifier book. */
export async function verifyRequest(vkey: object, p: Proven, rules: GatewayRules): Promise<Verdict> {
  let request: PublicRequest;
  try {
    request = readPublic(p.publicSignals);
  } catch {
    return { ok: false, reason: "malformed" };
  }
  if (request.epoch !== rules.epoch) return { ok: false, reason: "wrong epoch" };
  if (request.limit !== rules.limit) return { ok: false, reason: "wrong limit" };
  if (request.x !== rules.x) return { ok: false, reason: "proof is for a different message" };
  if (!rules.acceptedRoots.has(request.root)) return { ok: false, reason: "unknown or retired root" };
  const valid = await snarkjs.groth16.verify(vkey, p.publicSignals, p.proof);
  return valid ? { ok: true, request } : { ok: false, reason: "invalid proof" };
}

/** snarkjs keeps BN254 worker threads alive; call once when a Node process is done proving. */
export async function releaseProver(): Promise<void> {
  const curve = (globalThis as { curve_bn128?: { terminate(): Promise<void> } }).curve_bn128;
  if (curve) await curve.terminate();
}

export interface Share {
  x: bigint;
  y: bigint;
}

/** Two points of one slot's line y = secret + a1 * x give the secret. */
export function recoverSecret(a: Share, b: Share): bigint {
  if (mod(a.x - b.x) === 0n) throw new Error("same message: no second point");
  const a1 = mod((a.y - b.y) * inverse(a.x - b.x));
  return mod(a.y - a1 * a.x);
}

export type BookResult =
  | { status: "fresh" }
  | { status: "replay" }
  | { status: "double"; secret: bigint; nullifier: bigint };

/**
 * A gateway's record of spent tickets, in memory: the reference for what the book must decide. Tickets
 * never come back, so the book is never pruned. It must be checked and written in one step per request
 * (one process, or one atomic store) or two concurrent requests could both pass, and a gateway must keep
 * it across restarts: the live gateway appends every spent ticket to a file on its volume before it
 * answers and reloads that file when it starts, and publishes the list at /api/v1/used.
 */
export class NullifierBook {
  private readonly seen = new Map<bigint, Share>();

  /**
   * Records every active slot of a verified request. Any slot seen before with a different
   * message means a double use: nothing is recorded and the recovered secret is returned.
   * The same request sent again is a replay and is refused without recording anything.
   */
  record(request: PublicRequest): BookResult {
    const seen = this.seen;
    const active = request.nullifiers.slice(0, request.weight);
    for (let j = 0; j < active.length; j++) {
      const prior = seen.get(active[j]);
      if (!prior) continue;
      if (prior.x === request.x && prior.y === request.ys[j]) return { status: "replay" };
      return { status: "double", nullifier: active[j], secret: recoverSecret(prior, { x: request.x, y: request.ys[j] }) };
    }
    active.forEach((n, j) => seen.set(n, { x: request.x, y: request.ys[j] }));
    return { status: "fresh" };
  }

  get size(): number {
    return this.seen.size;
  }
}
