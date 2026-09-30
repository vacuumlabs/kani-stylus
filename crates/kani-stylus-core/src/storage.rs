//! A bounded symbolic model of the EVM storage trie.
//!
//! Stylus shares the EVM's storage: a `U256` slot key maps to a `B256` value.
//! This models it word for word, in two tiers:
//!
//! - **small slots** (below [`SMALL_SLOTS`]) are direct-indexed. These are a
//!   contract's own fields, whose slot numbers the SDK computes from
//!   constants, so an access resolves to one array cell during symbolic
//!   execution and never reaches the solver as a search;
//! - **every other slot** — mapping entries, keccak-derived bases — sits in a
//!   short association list, compared limb by limb.
//!
//! Both tiers hold exactly what a flat `slot -> word` map would hold, so the
//! split is an encoding choice, not a modelling one. It matters because the
//! flat list it replaces made every field access scan, and write into, the
//! whole list: on four `U256` fields read back, 4.9M clauses against 1.1M.
//! See `kb/36-storage-model.md`.
//!
//! The store is a global, like the keccak and arithmetic oracles, rather
//! than state behind an `Rc<RefCell<..>>` in the host. The SDK clones its
//! `Box<dyn Host>` on every storage access; with the state global, the host
//! is zero-sized and those clones cost nothing.
//!
//! An array rather than `std::collections::HashMap` is deliberate:
//! `HashMap::new()` seeds SipHash through a `getrandom` syscall, which Kani
//! cannot model — the reason the SDK's own `TestVM` cannot be verified.

use alloy_primitives::{B256, U256};

/// Slots below this are direct-indexed. A contract's own fields get
/// consecutive slots from zero, so this covers most contracts; a field beyond
/// it still works, it just lands in the association list.
///
/// Not free to raise. A write to a slot symbolic execution cannot prove
/// large — a keccak digest, under `precise-storage` — muxes over every cell.
/// On two symbolic mapping keys under `precise-storage`, 16, 32 and 64 cells
/// took 32.5s, 36.2s and 39.4s (1.39M, 1.63M and 2.10M clauses).
pub const SMALL_SLOTS: usize = 32;

/// Capacity of the association list. [`SymbolicVm`](crate::SymbolicVm)'s
/// `SLOTS` parameter bounds how much of it one proof may use. The scans stop
/// at `SLOTS`, so unused capacity adds nothing to the formula, but it is not
/// free: symbolic execution still tracks it, and on three symbolic mapping
/// keys 64 entries took 17.1s of symex against 11.0s for 16.
pub const MAX_SLOTS: usize = 32;

/// Storage as a plain value. [`crate::StorageSnapshot`] is a copy of one.
#[derive(Clone, Copy)]
pub(crate) struct Store {
    // Words are held as limbs, not as `B256`'s 32 bytes: symbolic execution
    // tracks each array element separately, so this is 8x fewer of them.
    small: [Word; SMALL_SLOTS],
    small_written: [bool; SMALL_SLOTS],
    keys: [Word; MAX_SLOTS],
    vals: [Word; MAX_SLOTS],
    /// What each listed slot held before the proof touched it: zero, or with
    /// arbitrary storage the value its first read drew. `changed_since`
    /// needs it for slots a snapshot never saw.
    inits: [Word; MAX_SLOTS],
    len: usize,
    arbitrary: bool,
    accessed: bool,
}

impl Store {
    pub(crate) const EMPTY: Self = Self {
        small: [ZERO; SMALL_SLOTS],
        small_written: [false; SMALL_SLOTS],
        keys: [ZERO; MAX_SLOTS],
        vals: [ZERO; MAX_SLOTS],
        inits: [ZERO; MAX_SLOTS],
        len: 0,
        arbitrary: false,
        accessed: false,
    };

    /// Unwritten slots read as an arbitrary value, fixed per slot, instead of
    /// zero: uninterpreted initial storage. Fails if storage was accessed.
    pub(crate) fn make_arbitrary(&mut self) {
        // Earlier reads would have seen zeros that the arbitrary values below
        // then contradict.
        assert!(
            !self.accessed,
            "kani-stylus: with_arbitrary_storage() must come before any storage access"
        );
        self.small = kani::any();
        self.arbitrary = true;
    }

    fn push<const SLOTS: usize>(&mut self, key: Word, val: Word, init: Word) {
        // A proof that needs more slots than `SLOTS` is verifying a smaller
        // contract than the one written, so fail loudly rather than dropping
        // the path.
        assert!(
            self.len < SLOTS,
            "kani-stylus: more than SLOTS distinct large storage slots -- raise SymbolicVm::<SLOTS>"
        );
        // Written position by position, up to `len`, so a symbolic `len` only
        // muxes over the positions it can take, not all of `MAX_SLOTS`.
        let mut j = 0;
        while j <= self.len && j < SLOTS {
            if j == self.len {
                self.keys[j] = key;
                self.vals[j] = val;
                self.inits[j] = init;
            }
            j += 1;
        }
        self.len += 1;
    }

    // `load` and `store` read and write the list inside the scan, at the
    // loop's own concrete index. Returning the index and using it afterwards
    // indexes the arrays with a symbolic value: on two structured mapping
    // keys, 12.1s and 0.51M clauses against 10.1s and 0.36M. (About even
    // under `precise-storage`.)

    pub(crate) fn load<const SLOTS: usize>(&mut self, key: U256) -> B256 {
        from_word(self.load_word::<SLOTS>(key))
    }

    fn load_word<const SLOTS: usize>(&mut self, key: U256) -> Word {
        self.accessed = true;
        let l = *key.as_limbs();
        if let Some(i) = small_index(&l) {
            return self.small[i];
        }
        let mut i = 0;
        while i < self.len && i < SLOTS {
            if limbs_eq(&self.keys[i], &l) {
                return self.vals[i];
            }
            i += 1;
        }
        if self.arbitrary {
            // Record the value, so every later read of this slot agrees.
            let v: Word = kani::any();
            self.push::<SLOTS>(l, v, v);
            return v;
        }
        // Unwritten slots read as zero, matching the EVM.
        ZERO
    }

    pub(crate) fn store<const SLOTS: usize>(&mut self, key: U256, value: B256) {
        self.accessed = true;
        let value = to_word(value);
        let l = *key.as_limbs();
        if let Some(i) = small_index(&l) {
            self.small[i] = value;
            self.small_written[i] = true;
            return;
        }
        let mut i = 0;
        while i < self.len && i < SLOTS {
            if limbs_eq(&self.keys[i], &l) {
                self.vals[i] = value;
                return;
            }
            i += 1;
        }
        // Never read, so it held its initial value: zero, or an arbitrary one
        // nobody has looked at, drawn now.
        let init = if self.arbitrary { kani::any() } else { ZERO };
        self.push::<SLOTS>(l, value, init);
    }

    /// Distinct slots written, plus, with arbitrary storage, large slots read.
    pub(crate) fn touched(&self) -> usize {
        let mut n = self.len;
        let mut i = 0;
        while i < SMALL_SLOTS {
            if self.small_written[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// Number of slots whose value differs from `before`.
    ///
    /// The basis of a *frame condition*. `before` is an earlier state of this
    /// same store, and the list only ever grows, so `before`'s large slots
    /// are a prefix of ours; a slot `before` never saw held its initial value
    /// at the time.
    pub(crate) fn changed_since<const SLOTS: usize>(&self, before: &Self) -> usize {
        let mut changed = 0;
        let mut i = 0;
        while i < SMALL_SLOTS {
            if !limbs_eq(&self.small[i], &before.small[i]) {
                changed += 1;
            }
            i += 1;
        }
        let mut j = 0;
        while j < self.len && j < SLOTS {
            let then = if j < before.len { before.vals[j] } else { self.inits[j] };
            if !limbs_eq(&self.vals[j], &then) {
                changed += 1;
            }
            j += 1;
        }
        changed
    }
}

fn small_index(l: &[u64; 4]) -> Option<usize> {
    if l[1] == 0 && l[2] == 0 && l[3] == 0 && l[0] < SMALL_SLOTS as u64 {
        Some(l[0] as usize)
    } else {
        None
    }
}

/// A storage word or slot key as four `u64`s. For keys these are `U256`'s
/// limbs; for values, `B256`'s bytes reinterpreted, since the store only ever
/// moves and compares them.
pub(crate) type Word = [u64; 4];

const ZERO: Word = [0; 4];

// SAFETY, both: `[u8; 32]` and `[u64; 4]` have the same size and every bit
// pattern is valid for each. By value, so alignment does not arise.
fn to_word(b: B256) -> Word {
    unsafe { core::mem::transmute(b.0) }
}

fn from_word(w: Word) -> B256 {
    B256::from(unsafe { core::mem::transmute::<Word, [u8; 32]>(w) })
}

// Limb-wise, because `==` on `U256` and `B256` lowers to `memcmp` over bytes,
// which costs far more to encode.
pub(crate) fn limbs_eq(a: &Word, b: &Word) -> bool {
    a[0] == b[0] && a[1] == b[1] && a[2] == b[2] && a[3] == b[3]
}

/// The one store. Kani gives each harness its own program, so this starts
/// empty for every proof; verification is single-threaded, so the
/// unsynchronised access is sound here.
static mut STORE: Store = Store::EMPTY;

pub(crate) fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    // SAFETY: see `STORE`.
    unsafe { f(&mut *core::ptr::addr_of_mut!(STORE)) }
}

/// The two-tier store is only an encoding: these check that it behaves
/// exactly like the flat `slot -> word` map it replaced, over every sequence
/// of a few operations on arbitrary slots, small and large alike.
///
/// `cargo kani -p kani-stylus-core`.
#[cfg(kani)]
mod proofs {
    use super::*;

    /// The reference: an association list, compared with plain `==`.
    struct Flat<const N: usize> {
        keys: [U256; N],
        vals: [B256; N],
        len: usize,
    }

    impl<const N: usize> Flat<N> {
        fn new() -> Self {
            Self { keys: [U256::ZERO; N], vals: [B256::ZERO; N], len: 0 }
        }
        fn load(&self, key: U256) -> B256 {
            let mut i = 0;
            while i < self.len {
                if self.keys[i] == key {
                    return self.vals[i];
                }
                i += 1;
            }
            B256::ZERO
        }
        fn store(&mut self, key: U256, value: B256) {
            let mut i = 0;
            while i < self.len {
                if self.keys[i] == key {
                    self.vals[i] = value;
                    return;
                }
                i += 1;
            }
            self.keys[self.len] = key;
            self.vals[self.len] = value;
            self.len += 1;
        }
    }

    /// One symbolic byte is enough to tell every write apart, and the store
    /// only moves values, never inspects them; the full 32 bytes made the
    /// refinement proof run past 19 minutes.
    fn any_word() -> B256 {
        let mut b = [0u8; 32];
        b[31] = kani::any();
        B256::from(b)
    }

    /// Slots from both tiers and the gap between them: a small slot, a
    /// structured-looking one, and one that is entirely arbitrary.
    fn any_slot() -> U256 {
        match kani::any::<u8>() % 3 {
            0 => U256::from(kani::any::<u8>() % (SMALL_SLOTS as u8 + 2)),
            1 => U256::from_limbs([kani::any::<u8>() as u64 % 3, 0, kani::any::<u8>() as u64 % 3, crate::slots::TAG]),
            _ => U256::from_limbs(kani::any()),
        }
    }

    #[kani::proof]
    fn two_tier_store_refines_a_flat_map() {
        const OPS: usize = 4;
        let mut s = Store::EMPTY;
        let mut f = Flat::<OPS>::new();
        let mut n = 0;
        while n < OPS {
            let key = any_slot();
            if kani::any() {
                let v = any_word();
                s.store::<OPS>(key, v);
                f.store(key, v);
            } else {
                assert!(s.load::<OPS>(key) == f.load(key), "store disagrees with the flat map");
            }
            n += 1;
        }
        let key = any_slot();
        assert!(s.load::<OPS>(key) == f.load(key), "store disagrees with the flat map");
        kani::cover!(s.len == 2 && f.len == 3, "both tiers in use");
    }

    /// With arbitrary storage, every read of a slot agrees with every other
    /// until it is written, and writes land.
    #[kani::proof]
    fn arbitrary_storage_is_consistent() {
        let mut s = Store::EMPTY;
        s.make_arbitrary();
        let (a, b) = (any_slot(), any_slot());
        let a0 = s.load::<4>(a);
        let b0 = s.load::<4>(b);
        assert!(s.load::<4>(a) == a0, "two reads of one slot disagree");
        let v = any_word();
        s.store::<4>(a, v);
        assert!(s.load::<4>(a) == v, "a write did not land");
        if a != b {
            assert!(s.load::<4>(b) == b0, "a write moved another slot");
        }
        kani::cover!(a != b && a0 != b0, "unwritten slots are not all alike");
    }

    /// `changed_since` counts exactly the slots whose value moved, including
    /// slots the snapshot never saw.
    #[kani::proof]
    fn changed_since_counts_what_moved() {
        let mut s = Store::EMPTY;
        if kani::any() {
            s.make_arbitrary();
        }
        let (a, b) = (any_slot(), any_slot());
        kani::assume(a != b);
        let a_before = s.load::<4>(a);
        let snap = s;
        let b_before = s.load::<4>(b); // first seen after the snapshot
        let (va, vb) = (any_word(), any_word());
        s.store::<4>(a, va);
        s.store::<4>(b, vb);
        let expected = (va != a_before) as usize + (vb != b_before) as usize;
        assert!(s.changed_since::<4>(&snap) == expected, "changed_since miscounted");
        kani::cover!(expected == 1, "one moved, one did not");
    }
}
