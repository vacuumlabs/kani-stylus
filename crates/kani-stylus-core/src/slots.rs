//! Mapping slots without keccak: storage layout as an injective function.
//!
//! A Stylus mapping puts `map[key]` at `keccak256(pad32(key) ‖ root)`, where
//! `root` is the map's own slot. A contract never sees that slot; only the
//! SDK's storage types do, and they use it for exactly two things: as a key
//! into storage, and as a base to add small offsets to (the fields of a
//! struct value, say). So the digest's *value* is irrelevant. What storage
//! correctness rests on is that distinct `(root, key)` pairs get disjoint
//! regions of slot space — the assumption Solidity's layout makes, and that
//! [`keccak`](crate::keccak) models with fresh digests assumed distinct.
//!
//! This module states the same thing more cheaply. The stubs below replace
//! the SDK's `StorageKey::to_slot` and number `(root, key)` pairs in order of
//! first use; pair `i` gets the slot
//!
//! ```text
//!     limbs [0, 0, i, TAG]   (little-endian), i.e. TAG·2^192 + i·2^128
//! ```
//!
//! Distinct pairs get distinct numbers, and a stride of 2^128 leaves every
//! entry room for any offset the SDK adds. Nothing is hashed, no digest is
//! assumed distinct from another, and the solver compares slots whose bits
//! are almost all constants. Measured on two symbolic keys: 10.1s against
//! 36.2s for the keccak oracle on the same store, 66.3s on the store it
//! replaced. See `kb/36-storage-model.md`.
//!
//! # When this applies
//!
//! Only to slot derivation. User code calling `stylus_sdk::crypto::keccak`
//! still goes through the keccak oracle, whose fresh digests are the right
//! model for a hash whose value a contract might inspect — a structured slot
//! would not be, since `TAG·2^192 + i·2^128` is a very particular number.
//!
//! Only for fixed-size key types whose `StorageKey` impl is not generic:
//! `Address`, `bool` and the primitive integers — and [`proof!`](crate::proof)
//! attaches only the unsigned ones, since Kani cannot take many more stub
//! attributes on one harness; see there. `U256`, `FixedBytes<N>` (so
//! `B256`) and `Signed` keys go through generic impls, which Kani 0.67 cannot
//! stub — the compiler crashes with "cannot find `BITS/#0` in param-env" — so
//! those maps keep the keccak oracle. So do byte-string keys, and the element
//! slots of `StorageVec` and `StorageBytes`, which hash the base slot
//! directly.
//!
//! # Assumptions
//!
//! 1. **Storage regions are disjoint.** Distinct `(root, key)` pairs never
//!    share a slot, and no offset into one region reaches another. This is
//!    the collision freedom every Solidity and Stylus contract's layout rests
//!    on. The cost, as with the keccak oracle: this cannot find a genuine
//!    storage-collision attack.
//! 2. **Digests avoid structured slots.** The keccak oracle assumes its
//!    digests never land in the `TAG` space, 2^-64 of all slots, so the two
//!    kinds of derived slot never alias spuriously.
//! 3. **At most [`MAX_ENTRIES`] distinct pairs per proof.** Exceeding it
//!    fails the proof loudly.
//!
//! None of these makes a counterexample spurious. Unlike
//! [`arith_oracle`](crate::arith_oracle), which knows only some facts about
//! `*` and `/`, this is an exact model of storage given assumption 1: the
//! contract behaves the same under any injective, region-disjoint layout.
//!
//! # Checking the abstraction
//!
//! Leave the `to_slot` stubs off and mappings fall back to hashing their real
//! preimages through the keccak oracle. [`proof!`](crate::proof) does that for
//! every harness when `kani-stylus-core`'s `precise-storage` feature is on.

use alloy_primitives::{Address, B256, U256};

use crate::storage::limbs_eq;

/// Top limb of every structured slot.
///
/// Well above any contract's own fields, and distinct from every keccak
/// digest by the oracle's assumption.
pub const TAG: u64 = 0x5354_5255_4354_5344; // "STRUCTSD"

/// Distinct `(root, key)` pairs one proof may derive slots for. Repeats are
/// free: they hit the table. Lookups scan only the entries in use, so a
/// generous bound costs little.
pub const MAX_ENTRIES: usize = 16;

struct Table {
    roots: [[u64; 4]; MAX_ENTRIES],
    keys: [[u64; 4]; MAX_ENTRIES],
    len: usize,
}

static mut TABLE: Table = Table {
    roots: [[0; 4]; MAX_ENTRIES],
    keys: [[0; 4]; MAX_ENTRIES],
    len: 0,
};

fn with_table<R>(f: impl FnOnce(&mut Table) -> R) -> R {
    // SAFETY: Kani gives each harness its own program and verifies sequential
    // code only, as for the store and the oracles.
    unsafe { f(&mut *core::ptr::addr_of_mut!(TABLE)) }
}

/// The structured slot for `key` in the map rooted at `root`. `key` is the
/// 32-byte word the SDK would hash, as limbs.
pub fn derive(root: B256, key: [u64; 4]) -> U256 {
    let root = *U256::from_be_bytes(root.0).as_limbs();
    with_table(|t| {
        let mut i = 0;
        while i < t.len {
            if limbs_eq(&t.roots[i], &root) && limbs_eq(&t.keys[i], &key) {
                return slot(i);
            }
            i += 1;
        }
        assert!(
            t.len < MAX_ENTRIES,
            "kani-stylus: more than MAX_ENTRIES distinct mapping entries -- raise slots::MAX_ENTRIES"
        );
        let n = t.len;
        t.roots[n] = root;
        t.keys[n] = key;
        t.len = n + 1;
        slot(n)
    })
}

fn slot(i: usize) -> U256 {
    U256::from_limbs([0, 0, i as u64, TAG])
}

/// Distinct mapping entries derived so far in this proof.
pub fn entries_derived() -> usize {
    with_table(|t| t.len)
}

/// Drop-in for `<Address as StorageKey>::to_slot`.
pub fn address_to_slot(key: &Address, root: B256) -> U256 {
    // The SDK hashes the address as a 160-bit integer, i.e. left-padded.
    let b = &key.0 .0;
    let lo = u64::from_be_bytes([b[12], b[13], b[14], b[15], b[16], b[17], b[18], b[19]]);
    let mid = u64::from_be_bytes([b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11]]);
    let hi = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as u64;
    derive(root, [lo, mid, hi, 0])
}

/// Drop-in for `<bool as StorageKey>::to_slot`.
pub fn bool_to_slot(key: &bool, root: B256) -> U256 {
    derive(root, [*key as u64, 0, 0, 0])
}

// The SDK widens every primitive to `U256` first, signed ones by
// reinterpreting as their unsigned twin -- `U256::from(*self as $uint)`.
macro_rules! int_to_slot {
    ($($name:ident: $t:ty => $u:ty;)*) => {$(
        #[doc = concat!("Drop-in for `<", stringify!($t), " as StorageKey>::to_slot`.")]
        pub fn $name(key: &$t, root: B256) -> U256 {
            derive(root, *U256::from(*key as $u).as_limbs())
        }
    )*};
}

int_to_slot! {
    u8_to_slot: u8 => u8;
    u16_to_slot: u16 => u16;
    u32_to_slot: u32 => u32;
    u64_to_slot: u64 => u64;
    u128_to_slot: u128 => u128;
    usize_to_slot: usize => usize;
    i8_to_slot: i8 => u8;
    i16_to_slot: i16 => u16;
    i32_to_slot: i32 => u32;
    i64_to_slot: i64 => u64;
    i128_to_slot: i128 => u128;
    isize_to_slot: isize => usize;
}

/// The two properties the structured layout rests on, checked for arbitrary
/// roots and keys: distinct pairs get distinct slots, and each entry's
/// region is out of reach of every other and of every small slot.
///
/// `cargo kani -p kani-stylus-core`.
#[cfg(kani)]
mod proofs {
    use super::*;

    #[kani::proof]
    fn structured_slots_are_injective_and_disjoint() {
        let (r1, r2) = (B256::from(kani::any::<[u8; 32]>()), B256::from(kani::any::<[u8; 32]>()));
        let (k1, k2): ([u64; 4], [u64; 4]) = (kani::any(), kani::any());
        let s1 = derive(r1, k1);
        let s2 = derive(r2, k2);
        let same_pair = r1 == r2 && k1 == k2;
        assert!((s1 == s2) == same_pair, "derive is not injective");

        // Any offset the SDK can add inside one region -- a struct field, an
        // inline array element -- stays inside it.
        let (o1, o2): (u128, u128) = (kani::any(), kani::any());
        let (e1, e2) = (s1 + U256::from(o1), s2 + U256::from(o2));
        if !same_pair {
            assert!(e1 != e2, "two entries' regions overlap");
        }
        assert!(e1 >= U256::from(crate::storage::SMALL_SLOTS), "an entry reaches the small slots");
        assert!(e1.as_limbs()[3] == TAG, "an entry leaves the structured space");
        kani::cover!(!same_pair && r1 == r2, "two keys of one map");
    }
}
