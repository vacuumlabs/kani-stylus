//! Formal verification for Arbitrum Stylus contracts, using
//! [Kani](https://model-checking.github.io/kani/).
//!
//! Stylus contracts talk to ArbOS through [`stylus_core::Host`]. This crate
//! supplies a **symbolic** implementation of that trait, so a single proof
//! harness covers every input at once instead of one concrete case per test.
//!
//! ```ignore
//! #[cfg(kani)]
//! mod proofs {
//!     use kani_stylus_core::SymbolicVM;
//!     use super::Counter;
//!
//!     #[kani::proof]
//!     fn adding_never_decreases_the_counter() {
//!         let vm = SymbolicVM::concrete_ctx();
//!         let mut c = Counter::from(&vm);
//!         let a = kani_stylus_core::any_u256();
//!         let b = kani_stylus_core::any_u256();
//!         c.set_number(a);
//!         c.add_number(b);
//!         assert!(c.number() >= a); // fails: U256 addition wraps
//!     }
//! }
//! ```
//!
//! # Why this crate has to exist
//!
//! The SDK already ships a mock host, `stylus_sdk::testing::TestVM`, and it
//! looks like the obvious thing to hand to Kani. It cannot be used:
//! `TestVM`'s state holds nine `std::HashMap`s, `HashMap::new()` seeds SipHash
//! from OS randomness via a `getrandom` syscall, and Kani cannot model foreign
//! functions. Verification aborts before reaching any contract code. Measured:
//! constructing a `TestVM` fails in 2.4s; binding a contract to one times out
//! at 420s. The same proof against `SymbolicVM` completes in 6s.
//!
//! This works because `stylus-proc` generates
//! `impl<H: Host + Clone + 'static> From<&H> for YourContract` — generic over
//! any host. `TestVM` is not privileged; it is merely the implementation that
//! ships.
//!
//! # Setup
//!
//! The contract crate needs the `stylus-test` feature for verification, and
//! only then. That feature is what makes the generic `From` impl exist and
//! makes `VM` hold a `Box<dyn Host>`; it does *not* mean you use `TestVM`. On
//! in a deployed build, it breaks every host call. So both go behind an opt-in
//! feature, and this crate is a regular optional dependency — `cargo kani`
//! builds the lib target, where dev-dependencies are not available:
//!
//! ```toml
//! [dependencies]
//! kani-stylus-core = { path = "../../crates/kani-stylus-core", optional = true }
//!
//! [features]
//! proofs = ["dep:kani-stylus-core", "stylus-sdk/stylus-test"]
//! ```
//!
//! Then `cargo kani --features proofs -Z stubbing --output-format terse`.
//!
//! # What is and isn't modelled
//!
//! Modelled: persistent storage (bounded; see [`storage`]), mapping slot
//! derivation (as an injective function — see [`slots`]), keccak256 (as an
//! uninterpreted injective function — see [`keccak`]), `msg`/`block`/`chain`
//! context.
//! Optionally, `U256` division exactly by its specification ([`arith`]), or
//! `*` and `/` as uninterpreted functions constrained by lemmas
//! ([`arith_oracle`]).
//!
//! Not modelled, and a proof reaching one of these **fails loudly** rather than
//! inventing an answer: cross-contract calls, `CREATE`/`CREATE2`, gas
//! accounting, and dispatch through the ABI router (proofs call contract
//! methods directly).
//!
//! # Two traps worth knowing before you trust a result
//!
//! **`U256` arithmetic wraps silently.** `alloy`/`ruint` define `+` and `-` as
//! `wrapping_add`/`wrapping_sub`, and Kani's automatic overflow checks only
//! cover primitive integers — not library types built on wrapping `u64` limbs.
//! A passing proof does **not** rule out overflow. State it explicitly, either
//! by assuming it away (`kani::assume(a.checked_add(b).is_some())`) or by
//! asserting the property you actually want (`assert!(result >= a)`).
//!
//! **Some bounds prune, so proofs can go vacuous.** Exceeding `SLOTS` or
//! [`slots::MAX_ENTRIES`] fails the proof, but exceeding the keccak oracle's
//! `MAX_HASHES` kills the path via `kani::assume(false)` — safe, but an
//! over-tight bound can leave nothing to check. If a proof passes implausibly
//! fast, add a `kani::cover` for the state you expect to reach.
//!
//! # Storage: structured by default, precise on request
//!
//! Write harnesses that touch mappings with [`proof!`]. It attaches the stubs
//! that give mapping entries structured slots ([`slots`]) instead of hashing
//! them, which is several times cheaper and grows more slowly with the
//! number of keys. Turn on this crate's `precise-storage` feature and the same
//! harnesses run the SDK's real slot derivation through the keccak oracle:
//!
//! ```bash
//! cargo kani --features proofs -Z stubbing                                  # structured
//! cargo kani --features proofs,kani-stylus-core/precise-storage -Z stubbing # precise
//! ```
//!
//! Why this is safe, what it assumes, and how Certora, hevm and Halmos do the
//! same: `kb/36-storage-model.md`.
//!
//! [`stylus_core::Host`]: https://docs.rs/stylus-core/0.10.9/stylus_core/host/trait.Host.html

#![cfg_attr(not(test), no_std)]

// Everything here depends on `kani::any()`, which exists only under `cfg(kani)`.
// Outside verification the crate is intentionally empty, so it can sit in a
// dependency list without affecting ordinary builds.
#[cfg(kani)]
extern crate alloc;

#[cfg(kani)]
pub mod arith;
// The one exception: `arith_oracle`'s lemmas are plain predicates, and
// `cargo test` checks them against real ruint arithmetic. Only its stubs need
// Kani.
#[cfg(any(kani, test))]
pub mod arith_oracle;
#[cfg(kani)]
pub mod context;
#[cfg(kani)]
pub mod host;
#[cfg(kani)]
pub mod keccak;
#[cfg(kani)]
pub mod slots;
#[cfg(kani)]
pub mod storage;

#[cfg(kani)]
pub use arith::{
    any_uint, div_rem_memo, div_rem_monotone, div_rem_spec, split, ueq, ult, widen,
    wrapping_div_stub, wrapping_div_stub_memo, wrapping_div_stub_monotone, MAX_DIVS,
};
#[cfg(kani)]
pub use context::{any_address, any_u256, Context};
#[cfg(kani)]
pub use host::{StorageSnapshot, SymbolicVM, SymbolicVm};
#[cfg(kani)]
pub use keccak::{keccak_stub, HashOracle};

/// Declare Kani harnesses with kani-stylus's storage model attached.
///
/// ```ignore
/// kani_stylus_core::proof! {
///     /// Any attributes go through, `#[kani::should_panic]` and extra
///     /// `#[kani::stub]`s included.
///     fn balances_do_not_alias() {
///         // ...
///     }
///
///     fn another_harness() { /* ... */ }
/// }
/// ```
///
/// Each function becomes a `#[kani::proof]` with the keccak oracle stubbed
/// in, plus — unless this crate's `precise-storage` feature is on — the
/// structured slot derivation of [`slots`] for `Address`, `bool` and unsigned
/// integer keys. Run with `-Z stubbing`. [`precise_proof!`] always leaves the
/// slot derivation real, for a harness that must.
///
/// Signed integer keys are left to the keccak oracle, although [`slots`] has
/// their stubs: Kani expands each `#[kani::stub]` one level deeper than the
/// last, and past about fourteen Kani attributes on one function rustc's
/// default `recursion_limit` of 128 is exhausted. Nine leaves room for a
/// harness's own — `#[kani::should_panic]` and the two
/// [`arith_oracle`] stubs, say. A harness that needs them can list them itself.
#[cfg(not(feature = "precise-storage"))]
#[macro_export]
macro_rules! proof {
    ($($(#[$m:meta])* fn $name:ident() $body:block)*) => {$(
        #[kani::proof]
        #[kani::stub(stylus_sdk::crypto::keccak, kani_stylus_core::keccak_stub)]
        #[kani::stub(<stylus_sdk::alloy_primitives::Address as stylus_sdk::storage::StorageKey>::to_slot, kani_stylus_core::slots::address_to_slot)]
        #[kani::stub(<bool as stylus_sdk::storage::StorageKey>::to_slot, kani_stylus_core::slots::bool_to_slot)]
        #[kani::stub(<u8 as stylus_sdk::storage::StorageKey>::to_slot, kani_stylus_core::slots::u8_to_slot)]
        #[kani::stub(<u16 as stylus_sdk::storage::StorageKey>::to_slot, kani_stylus_core::slots::u16_to_slot)]
        #[kani::stub(<u32 as stylus_sdk::storage::StorageKey>::to_slot, kani_stylus_core::slots::u32_to_slot)]
        #[kani::stub(<u64 as stylus_sdk::storage::StorageKey>::to_slot, kani_stylus_core::slots::u64_to_slot)]
        #[kani::stub(<u128 as stylus_sdk::storage::StorageKey>::to_slot, kani_stylus_core::slots::u128_to_slot)]
        #[kani::stub(<usize as stylus_sdk::storage::StorageKey>::to_slot, kani_stylus_core::slots::usize_to_slot)]
        $(#[$m])*
        fn $name() $body
    )*};
}

/// Declare Kani harnesses with kani-stylus's storage model attached — the
/// precise variant, selected by this crate's `precise-storage` feature.
#[cfg(feature = "precise-storage")]
#[macro_export]
macro_rules! proof {
    ($($t:tt)*) => { kani_stylus_core::precise_proof! { $($t)* } };
}

/// Like [`proof!`], but mapping slots are always derived by the SDK itself,
/// hashing through the keccak oracle. This is what `precise-storage` makes
/// every [`proof!`] do.
#[macro_export]
macro_rules! precise_proof {
    ($($(#[$m:meta])* fn $name:ident() $body:block)*) => {$(
        #[kani::proof]
        #[kani::stub(stylus_sdk::crypto::keccak, kani_stylus_core::keccak_stub)]
        $(#[$m])*
        fn $name() $body
    )*};
}
