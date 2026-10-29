pragma circom 2.1.9;

include "../node_modules/circomlib/circuits/poseidon.circom";
include "../node_modules/circomlib/circuits/comparators.circom";
include "../node_modules/circomlib/circuits/bitify.circom";
include "../node_modules/circomlib/circuits/mux1.circom";

// One PARANOID request: "some member of the vault, paid up through this epoch, spends `weight`
// unused slots of this epoch's allowance on the message hashed to `x`".
//
// Rate-limit nullifiers (RLN v2): every slot id in [0, limit) gives the member one line
//   y = secret + a1 * x,   a1 = Poseidon(secret, Poseidon(epoch, APP), id)
// and one nullifier Poseidon(a1). A slot used once shows one point of its line and nothing
// about `secret`; the same slot used for two different messages shows two points, and anyone
// holding both recovers `secret`, which the program accepts as proof of the double use.
//
// The slot window [start, start + weight) stays private: a public counter would let the gateway
// chain one member's requests together. Unused outputs (j >= weight) are pinned to zero.
template Request(DEPTH, W) {
    // Public inputs
    signal input root;
    signal input epoch;
    signal input x;
    signal input limit;
    signal input weight;

    // Private inputs
    signal input secret;
    signal input endEpoch;
    signal input start;
    signal input pathElements[DEPTH];
    signal input pathIndices[DEPTH];

    signal output nullifiers[W];
    signal output ys[W];

    var APP = 0x706172616e6f69642e7631; // "paranoid.v1"

    // Membership: leaf = Poseidon(Poseidon(secret), endEpoch) is in the tree under `root`.
    component commitment = Poseidon(1);
    commitment.inputs[0] <== secret;
    component leaf = Poseidon(2);
    leaf.inputs[0] <== commitment.out;
    leaf.inputs[1] <== endEpoch;

    component levels[DEPTH];
    component order[DEPTH];
    signal node[DEPTH + 1];
    node[0] <== leaf.out;
    for (var i = 0; i < DEPTH; i++) {
        pathIndices[i] * (pathIndices[i] - 1) === 0;
        order[i] = MultiMux1(2);
        order[i].c[0][0] <== node[i];
        order[i].c[0][1] <== pathElements[i];
        order[i].c[1][0] <== pathElements[i];
        order[i].c[1][1] <== node[i];
        order[i].s <== pathIndices[i];
        levels[i] = Poseidon(2);
        levels[i].inputs[0] <== order[i].out[0];
        levels[i].inputs[1] <== order[i].out[1];
        node[i + 1] <== levels[i].out;
    }
    root === node[DEPTH];

    // Paid up: epoch <= endEpoch. Both fit in 32 bits (the program stores u32 epochs).
    component epochBits = Num2Bits(32);
    epochBits.in <== epoch;
    component endBits = Num2Bits(32);
    endBits.in <== endEpoch;
    component paid = LessEqThan(32);
    paid.in[0] <== epoch;
    paid.in[1] <== endEpoch;
    paid.out === 1;

    // Allowance: 1 <= weight <= W and start + weight <= limit < 2^16.
    component startBits = Num2Bits(16);
    startBits.in <== start;
    component limitBits = Num2Bits(16);
    limitBits.in <== limit;
    component weightBits = Num2Bits(8);
    weightBits.in <== weight;
    component weightLow = GreaterEqThan(8);
    weightLow.in[0] <== weight;
    weightLow.in[1] <== 1;
    weightLow.out === 1;
    component weightHigh = LessEqThan(8);
    weightHigh.in[0] <== weight;
    weightHigh.in[1] <== W;
    weightHigh.out === 1;
    component within = LessEqThan(17);
    within.in[0] <== start + weight;
    within.in[1] <== limit;
    within.out === 1;

    component external = Poseidon(2);
    external.inputs[0] <== epoch;
    external.inputs[1] <== APP;

    component a1[W];
    component nul[W];
    component active[W];
    signal line[W];
    for (var j = 0; j < W; j++) {
        active[j] = LessThan(8);
        active[j].in[0] <== j;
        active[j].in[1] <== weight;

        a1[j] = Poseidon(3);
        a1[j].inputs[0] <== secret;
        a1[j].inputs[1] <== external.out;
        a1[j].inputs[2] <== start + j;
        nul[j] = Poseidon(1);
        nul[j].inputs[0] <== a1[j].out;

        nullifiers[j] <== active[j].out * nul[j].out;
        line[j] <== secret + a1[j].out * x;
        ys[j] <== active[j].out * line[j];
    }
}

component main {public [root, epoch, x, limit, weight]} = Request(20, 4);
