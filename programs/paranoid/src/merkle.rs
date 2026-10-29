//! Poseidon (BN254, x^5, circomlib parameters) through the Solana syscall, and the member tree.
//! Hashes match circomlib's `Poseidon(n)` used by circuits/request.circom, big-endian encoded.

use anchor_lang::prelude::*;
use solana_poseidon::{hashv, Endianness, Parameters};

use crate::constants::{ROOT_HISTORY, TREE_DEPTH};
use crate::error::ParanoidError;
use crate::state::{RootEntry, Tree};

pub type Node = [u8; 32];

pub fn poseidon(inputs: &[&[u8]]) -> Result<Node> {
    hashv(Parameters::Bn254X5, Endianness::BigEndian, inputs)
        .map(|h| h.to_bytes())
        .map_err(|_| error!(ParanoidError::NotAFieldElement))
}

pub fn field_u32(value: u32) -> Node {
    let mut out = [0u8; 32];
    out[28..].copy_from_slice(&value.to_be_bytes());
    out
}

pub fn field_u64(value: u64) -> Node {
    let mut out = [0u8; 32];
    out[24..].copy_from_slice(&value.to_be_bytes());
    out
}

/// leaf = Poseidon(commitment, end_epoch, questions), as in circuits/request.circom.
pub fn leaf(commitment: &Node, end_epoch: u32, questions: u64) -> Result<Node> {
    poseidon(&[commitment, &field_u32(end_epoch), &field_u64(questions)])
}

impl Tree {
    pub fn init(&mut self) -> Result<()> {
        self.zeros[0] = [0u8; 32];
        for i in 0..TREE_DEPTH {
            self.zeros[i + 1] = poseidon(&[&self.zeros[i], &self.zeros[i]])?;
            self.filled[i] = self.zeros[i];
        }
        self.next_index = 0;
        self.head = 0;
        self.roots[0] = RootEntry { root: self.zeros[TREE_DEPTH], valid_before: u32::MAX };
        Ok(())
    }

    pub fn root(&self) -> Node {
        self.roots[self.head as usize].root
    }

    pub fn insert(&mut self, leaf: Node) -> Result<u32> {
        let index = self.next_index;
        require!((index as u64) < (1u64 << TREE_DEPTH), ParanoidError::TreeFull);
        let mut position = index;
        let mut node = leaf;
        for i in 0..TREE_DEPTH {
            node = if position & 1 == 0 {
                self.filled[i] = node;
                poseidon(&[&node, &self.zeros[i]])?
            } else {
                poseidon(&[&self.filled[i], &node])?
            };
            position >>= 1;
        }
        self.next_index = index + 1;
        self.push_root(node);
        Ok(index)
    }

    /// Replaces the leaf at `index`, proven by `siblings` against the current root, and keeps the
    /// insertion frontier consistent so later inserts hash over the edited subtree.
    pub fn replace(&mut self, index: u32, old_leaf: Node, siblings: &[Node], new_leaf: Node) -> Result<Node> {
        require!(index < self.next_index && siblings.len() == TREE_DEPTH, ParanoidError::InvalidPath);
        let last = self.next_index - 1;
        let mut old = old_leaf;
        let mut new = new_leaf;
        for (i, sibling) in siblings.iter().enumerate() {
            let position = index >> i;
            if position == (last >> i) & !1 {
                self.filled[i] = new;
            }
            (old, new) = if position & 1 == 0 {
                (poseidon(&[&old, sibling])?, poseidon(&[&new, sibling])?)
            } else {
                (poseidon(&[sibling, &old])?, poseidon(&[sibling, &new])?)
            };
        }
        require!(old == self.root(), ParanoidError::InvalidPath);
        Ok(new)
    }

    pub fn push_root(&mut self, root: Node) {
        self.head = (self.head + 1) % ROOT_HISTORY as u32;
        self.roots[self.head as usize] = RootEntry { root, valid_before: u32::MAX };
    }

    /// Stops every stored root from being accepted from epoch `valid_before` on. Called before
    /// pushing the root that no longer contains a removed leaf, so only older roots are capped.
    pub fn cap_roots(&mut self, valid_before: u32) {
        for entry in self.roots.iter_mut() {
            entry.valid_before = entry.valid_before.min(valid_before);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anchor_lang::__private::bytemuck::Zeroable;
    use proptest::prelude::*;
    use std::collections::BTreeMap;

    /// Root of a sparse tree recomputed from scratch, and the sibling path of `index`.
    fn reference(leaves: &BTreeMap<u32, Node>, zeros: &[Node], index: u32) -> (Node, Vec<Node>) {
        let mut level: BTreeMap<u32, Node> = leaves.clone();
        let mut path = Vec::new();
        for (i, zero) in zeros.iter().enumerate().take(TREE_DEPTH) {
            let p = index >> i;
            path.push(*level.get(&(p ^ 1)).unwrap_or(zero));
            let mut up = BTreeMap::new();
            for &k in level.keys() {
                let parent = k >> 1;
                if up.contains_key(&parent) {
                    continue;
                }
                let l = *level.get(&(parent * 2)).unwrap_or(zero);
                let r = *level.get(&(parent * 2 + 1)).unwrap_or(zero);
                up.insert(parent, poseidon(&[&l, &r]).unwrap());
            }
            level = up;
        }
        (*level.get(&0).unwrap_or(&zeros[TREE_DEPTH]), path)
    }

    fn fresh() -> Box<Tree> {
        let mut t = Box::new(Tree::zeroed());
        t.init().unwrap();
        t
    }

    #[test]
    fn empty_root_matches_circomlib() {
        // circomlibjs: zeros[1] = poseidon([0, 0])
        let t = fresh();
        let expected = "2098f5fb9e239eab3ceac3f27b81e481dc3124d55ffed523a839ee8446b64864";
        let got: String = t.zeros[1].iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(got, expected);
    }

    #[test]
    fn replace_rejects_a_wrong_path() {
        let mut t = fresh();
        t.insert(field_u32(7)).unwrap();
        let mut bad = vec![[0u8; 32]; TREE_DEPTH];
        bad[3] = field_u32(1);
        assert!(t.replace(0, field_u32(7), &bad, [0u8; 32]).is_err());
        assert!(t.replace(1, field_u32(7), &vec![[0u8; 32]; TREE_DEPTH], [0u8; 32]).is_err());
        assert!(t.replace(0, field_u32(7), &[[0u8; 32]; 3], [0u8; 32]).is_err());
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(24))]
        /// Any mix of inserts and removals leaves the same root as recomputing the tree.
        #[test]
        fn edits_match_recomputation(ops in proptest::collection::vec((any::<bool>(), any::<u16>()), 1..40)) {
            let mut t = fresh();
            let mut leaves = BTreeMap::new();
            for (n, (insert, pick)) in ops.into_iter().enumerate() {
                if insert || leaves.is_empty() {
                    let leaf = field_u32(1_000 + n as u32);
                    let index = t.insert(leaf).unwrap();
                    leaves.insert(index, leaf);
                } else {
                    let keys: Vec<u32> = leaves.keys().copied().collect();
                    let index = keys[pick as usize % keys.len()];
                    let (_, path) = reference(&leaves, &t.zeros, index);
                    let root = t.replace(index, leaves[&index], &path, [0u8; 32]).unwrap();
                    t.push_root(root);
                    leaves.remove(&index);
                }
                let (expected, _) = reference(&leaves, &t.zeros, 0);
                prop_assert_eq!(t.root(), expected);
            }
        }
    }
}
