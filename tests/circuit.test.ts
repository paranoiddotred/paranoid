// The request circuit (v2, question packs) and the gateway checks, without a chain: what verifies,
// what cannot be proven at all, and how a ticket used twice, on any day, turns into the secret.
import { after, test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  FIELD, MemberTree, NullifierBook, SLOTS, commitmentOf, leafOf, loadHash, messageHash,
  proveRequest, randomSecret, readPublic, recoverSecret, releaseProver, verifyRequest, type Proven, type RequestWitness,
} from "../sdk/rln.ts";

const artifacts = { wasm: "build/request_js/request.wasm", zkey: "build/request.zkey" };
const vkey = JSON.parse(readFileSync("build/verification_key.json", "utf8"));
const hash = await loadHash();
const enc = new TextEncoder();
after(releaseProver);

const alice = randomSecret();
const bob = randomSecret();
const tree = new MemberTree(hash);
tree.set(0, leafOf(hash, commitmentOf(hash, bob), 50, 300));
tree.set(3, leafOf(hash, commitmentOf(hash, alice), 40, 8));

const base = async (over: Partial<RequestWitness> = {}): Promise<RequestWitness> => ({
  secret: alice, endEpoch: 40, questions: 8, leafIndex: 3, tree, epoch: 12, start: 0, weight: 1, limit: 1000,
  x: await messageHash(enc.encode("what is a nullifier?")), ...over,
});
const rulesFor = (w: RequestWitness) => ({ acceptedRoots: new Set([tree.root()]), epoch: w.epoch, limit: w.limit, x: w.x });

async function cannotProve(w: RequestWitness) {
  await assert.rejects(proveRequest(w, artifacts));
}

test("a member's proof verifies and exposes only ticket nullifiers and shares", async () => {
  const w = await base({ weight: 3, start: 2 });
  const p = await proveRequest(w, artifacts);
  const v = await verifyRequest(vkey, p, rulesFor(w));
  assert.equal(v.ok, true);
  const r = readPublic(p.publicSignals);
  assert.equal(r.nullifiers.filter((n) => n !== 0n).length, 3);
  assert.deepEqual(r.nullifiers.slice(3), [0n]);
  assert.deepEqual(r.ys.slice(3), [0n]);
  assert.equal(new Set(r.nullifiers.slice(0, 3)).size, 3);
  for (const y of r.ys.slice(0, 3)) assert.notEqual(y, alice);
});

test("every weight from 1 to SLOTS proves; the last ticket bought is usable", async () => {
  for (let weight = 1; weight <= SLOTS; weight++) {
    const w = await base({ weight, start: 8 - weight });
    assert.equal((await verifyRequest(vkey, await proveRequest(w, artifacts), rulesFor(w))).ok, true);
  }
});

test("no proof exists past the tickets bought, past the last valid day, for weight 0 or above SLOTS", async () => {
  await cannotProve(await base({ start: 7, weight: 2 }));
  await cannotProve(await base({ start: 8, weight: 1 }));
  await cannotProve(await base({ epoch: 41 }));
  await cannotProve(await base({ weight: 0 }));
  await cannotProve(await base({ weight: SLOTS + 1, limit: 64 }));
  await cannotProve(await base({ start: 1 << 16, limit: 65535 }));
});

test("no proof exists above the protocol cap, even for a member whose leaf says so", async () => {
  await cannotProve(await base({ limit: 7 }));
});

test("no proof exists for a secret, last day or ticket count that is not in the tree", async () => {
  await cannotProve(await base({ secret: randomSecret() }));
  await cannotProve(await base({ endEpoch: 41 }));
  await cannotProve(await base({ questions: 9, start: 8 }));
  await cannotProve(await base({ leafIndex: 0 }));
});

test("the gateway refuses a changed message, epoch, limit, root or any tampered signal", async () => {
  const w = await base();
  const p = await proveRequest(w, artifacts);
  const rules = rulesFor(w);
  const other = await messageHash(enc.encode("a different prompt"));
  assert.equal((await verifyRequest(vkey, p, { ...rules, x: other })).ok, false);
  assert.equal((await verifyRequest(vkey, p, { ...rules, epoch: 13 })).ok, false);
  assert.equal((await verifyRequest(vkey, p, { ...rules, limit: 9 })).ok, false);
  assert.equal((await verifyRequest(vkey, p, { ...rules, acceptedRoots: new Set([1n]) })).ok, false);
  for (let i = 0; i < p.publicSignals.length; i++) {
    const signals = [...p.publicSignals];
    signals[i] = ((BigInt(signals[i]) + 1n) % FIELD).toString();
    const tampered: Proven = { proof: p.proof, publicSignals: signals };
    const r = readPublic(signals);
    const v = await verifyRequest(vkey, tampered, { acceptedRoots: new Set([r.root]), epoch: r.epoch, limit: r.limit, x: r.x });
    assert.equal(v.ok, false, `signal ${i}`);
  }
});

test("one ticket used for two messages gives the secret back; a resend is only a replay", async () => {
  const book = new NullifierBook();
  const first = await proveRequest(await base({ start: 4, weight: 2 }), artifacts);
  assert.equal(book.record(readPublic(first.publicSignals)).status, "fresh");
  assert.equal(book.record(readPublic(first.publicSignals)).status, "replay");

  const x2 = await messageHash(enc.encode("second prompt on a reused slot"));
  const second = await proveRequest(await base({ start: 5, weight: 1, x: x2 }), artifacts);
  const result = book.record(readPublic(second.publicSignals));
  assert.equal(result.status, "double");
  assert.equal(result.status === "double" && result.secret, alice);
  assert.equal(hash([alice]), commitmentOf(hash, alice));
});

test("a ticket never comes back: the same ticket on a later day is a double use", async () => {
  const book = new NullifierBook();
  const today = await proveRequest(await base({ epoch: 12, start: 3, weight: 1 }), artifacts);
  assert.equal(book.record(readPublic(today.publicSignals)).status, "fresh");
  const x2 = await messageHash(enc.encode("tomorrow, same ticket"));
  const tomorrow = await proveRequest(await base({ epoch: 13, start: 3, weight: 1, x: x2 }), artifacts);
  assert.equal(readPublic(tomorrow.publicSignals).nullifiers[0], readPublic(today.publicSignals).nullifiers[0]);
  const result = book.record(readPublic(tomorrow.publicSignals));
  assert.equal(result.status === "double" && result.secret, alice);
});

test("different tickets and different members never collide", async () => {
  const book = new NullifierBook();
  const x = await messageHash(enc.encode("same words"));
  const runs: Partial<RequestWitness>[] = [
    { start: 0, weight: 4, x }, { start: 4, weight: 4, x },
    { secret: bob, endEpoch: 50, questions: 300, leafIndex: 0, start: 0, weight: 4, x },
    { secret: bob, endEpoch: 50, questions: 300, leafIndex: 0, start: 296, weight: 4, x },
  ];
  for (const r of runs) assert.equal(book.record(readPublic((await proveRequest(await base(r), artifacts)).publicSignals)).status, "fresh");
  assert.equal(book.size, 16);
});

test("secret recovery is exact field arithmetic", () => {
  const secret = randomSecret();
  const a1 = randomSecret();
  const at = (x: bigint) => ({ x, y: (secret + a1 * x) % FIELD });
  assert.equal(recoverSecret(at(5n), at(FIELD - 3n)), secret);
  assert.throws(() => recoverSecret(at(7n), at(7n)));
});
