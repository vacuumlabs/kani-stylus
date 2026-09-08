//! keccak256 modelled as an uninterpreted injective function.
//!
//! A faithful, bit-level keccak256 is hopeless for an SMT solver — it is
//! precisely the kind of function designed to defeat one. But contracts almost
//! never depend on the *value* of a hash; they depend on two of its properties:
//!
//! - **determinism** — the same preimage always gives the same digest, so a
//!   mapping entry written under key `k` is read back under key `k`;
//! - **injectivity** — different preimages give different digests, so distinct
//!   mapping entries occupy distinct slots and don't alias.
//!
//! So we model exactly those two properties and nothing else. Each distinct
//! preimage is assigned a fresh symbolic digest, constrained only to differ
//! from every digest handed out before it. The solver never sees a round of
//! keccak.
//!
//! # You must stub, or this is never called
//!
//! Stylus mappings do **not** route hashing through the `Host` trait.
//! `stylus_sdk::storage::map` calls `crypto::keccak`, which calls
//! `alloy_primitives::keccak256` directly. Implementing
//! `CryptographyAccess::native_keccak256` is therefore not enough — real
//! keccak would still reach the solver.
//!
//! Any harness that touches a mapping needs the stub:
//!
//! ```ignore
//! #[kani::proof]
//! #[kani::stub(stylus_sdk::crypto::keccak, kani_stylus_core::keccak_stub)]
//! fn balances_do_not_alias() { /* ... */ }
//! ```
//!
//! and the run needs `cargo kani -Z stubbing`.
//!
//! The oracle is global rather than per-VM, which is also the semantically
//! honest choice: keccak256 is one pure function, not a property of a
//! particular host instance.
//!
//! # Modelling assumptions
//!
//! These are assumptions, not theorems. A proof built on this oracle holds
//! *given* that keccak256 behaves like an injective function — which is what
//! every hand proof about Solidity storage layout also assumes. Concretely:
//!
//! 1. **Collision freedom is assumed, not proved.** A real keccak collision
//!    would invalidate proofs built on this model. Finding one is a
//!    cryptographic break, so this is the standard and accepted trade.
//! 2. **Digests are assumed not to alias low storage slots** (see
//!    [`MIN_DIGEST_SLOT`]). Without this the solver will happily propose a
//!    digest equal to `0` or `1` and "find" a bug where a mapping entry
//!    collides with a scalar field. Such collisions are astronomically
//!    improbable in reality, and admitting them produces false positives that
//!    drown the real findings. The cost: this oracle **cannot find genuine
//!    storage-collision attacks**. That needs a different model.
//! 3. **Preimages are bounded** to [`MAX_PREIMAGE`] bytes and distinct hashes
//!    to [`MAX_HASHES`]. Exceeding either prunes the path via
//!    `kani::assume(false)` rather than wrapping, so a proof can never quietly
//!    check *less* than it claims — but it can become vacuous. If a proof
//!    passes implausibly fast, check reachability with `kani::cover`.

use alloy_primitives::{B256, U256};

/// Longest preimage the oracle will hash.
///
/// 64 bytes covers the shapes that matter: `keccak256(key ‖ slot)` for a
/// mapping entry and `keccak256(slot)` for a dynamic array base.
pub const MAX_PREIMAGE: usize = 64;

/// Distinct preimages one proof may hash. Repeats are free — they hit the memo.
pub const MAX_HASHES: usize = 8;

/// Digests are constrained to be at least this large when read as a slot index,
/// so they cannot alias the low slots holding scalar fields.
///
/// See assumption 2 in the module docs.
pub const MIN_DIGEST_SLOT: u64 = 1 << 32;

/// Bounded memo table implementing the injective-function model.
pub struct HashOracle {
    preimages: [[u8; MAX_PREIMAGE]; MAX_HASHES],
    lens: [usize; MAX_HASHES],
    digests: [B256; MAX_HASHES],
    len: usize,
}

impl HashOracle {
    pub const fn new() -> Self {
        Self {
            preimages: [[0u8; MAX_PREIMAGE]; MAX_HASHES],
            lens: [0usize; MAX_HASHES],
            digests: [B256::ZERO; MAX_HASHES],
            len: 0,
        }
    }

    /// Distinct preimages hashed so far. Compare against [`MAX_HASHES`] in a
    /// proof to confirm the bound isn't silently binding.
    pub fn distinct_hashes(&self) -> usize {
        self.len
    }

    pub fn hash(&mut self, input: &[u8]) -> B256 {
        // Longer preimages aren't modelled. Prune rather than truncate: a
        // truncating oracle would break injectivity and could mask a real bug.
        kani::assume(input.len() <= MAX_PREIMAGE);

        // Determinism: an already-seen preimage returns its recorded digest.
        let mut i = 0;
        while i < self.len {
            if self.lens[i] == input.len() && prefix_eq(&self.preimages[i], input) {
                return self.digests[i];
            }
            i += 1;
        }

        // A new preimage. Draw a fresh digest and constrain it to differ from
        // every digest already issued — this *is* the injectivity assumption.
        let digest = B256::from(kani::any::<[u8; 32]>());
        let mut j = 0;
        while j < self.len {
            kani::assume(self.digests[j] != digest);
            j += 1;
        }
        // ...and to stay clear of the low slots used by scalar fields.
        kani::assume(U256::from_be_bytes(digest.0) >= U256::from(MIN_DIGEST_SLOT));

        // Prune paths that would exceed the table rather than overwrite.
        kani::assume(self.len < MAX_HASHES);
        let n = self.len;
        let mut k = 0;
        while k < input.len() {
            self.preimages[n][k] = input[k];
            k += 1;
        }
        self.lens[n] = input.len();
        self.digests[n] = digest;
        self.len = n + 1;
        digest
    }
}

impl Default for HashOracle {
    fn default() -> Self {
        Self::new()
    }
}

/// The one oracle, shared by the stub and by `Host::native_keccak256` so the
/// two always agree.
///
/// Kani gives each harness its own program entry, so this starts empty for
/// every proof. Verification is single-threaded, so the unsynchronised access
/// below is sound here in a way it would not be in production code.
static mut GLOBAL_ORACLE: HashOracle = HashOracle::new();

fn with_oracle<R>(f: impl FnOnce(&mut HashOracle) -> R) -> R {
    // SAFETY: Kani verifies sequential code only — there is no concurrent
    // access to reason about. See the note on `GLOBAL_ORACLE`.
    unsafe { f(&mut *core::ptr::addr_of_mut!(GLOBAL_ORACLE)) }
}

/// Drop-in replacement for `stylus_sdk::crypto::keccak`.
///
/// Use it via `#[kani::stub(stylus_sdk::crypto::keccak,
/// kani_stylus_core::keccak_stub)]`, and run with `cargo kani -Z stubbing`.
/// The signature deliberately mirrors the original, including its single
/// generic parameter — Kani requires the arity and generic count to match.
pub fn keccak_stub<T: AsRef<[u8]>>(bytes: T) -> B256 {
    with_oracle(|o| o.hash(bytes.as_ref()))
}

/// Hash through the shared oracle. Used by `Host::native_keccak256`.
pub fn oracle_hash(input: &[u8]) -> B256 {
    with_oracle(|o| o.hash(input))
}

/// Distinct preimages hashed so far in this proof.
pub fn hashes_taken() -> usize {
    with_oracle(|o| o.distinct_hashes())
}

/// Compare the first `input.len()` bytes of a fixed buffer against a slice.
///
/// An explicit loop, not `&buf[..n] == input`: slice equality goes through
/// `memcmp`, which drags SIMD comparison code into the goto program for no
/// benefit at these sizes.
fn prefix_eq(buf: &[u8; MAX_PREIMAGE], input: &[u8]) -> bool {
    let mut i = 0;
    while i < input.len() {
        if buf[i] != input[i] {
            return false;
        }
        i += 1;
    }
    true
}
