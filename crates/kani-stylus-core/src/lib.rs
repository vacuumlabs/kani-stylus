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
//! The contract crate needs the `stylus-test` feature. That feature is what
//! makes the generic `From` impl exist and makes `VM` hold a `Box<dyn Host>`;
//! it does *not* mean you use `TestVM`.
//!
//! ```toml
//! [dependencies]
//! stylus-sdk = { version = "0.10.9", features = ["stylus-test"] }
//!
//! [dev-dependencies]
//! kani-stylus-core = { path = "../../crates/kani-stylus-core" }
//! ```
//!
//! Then `cargo kani --output-format terse`.
//!
//! # What is and isn't modelled
//!
//! Modelled: persistent storage (bounded), keccak256 (as an uninterpreted
//! injective function — see [`keccak`]), `msg`/`block`/`chain` context.
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
//! **Bounds prune, so proofs can go vacuous.** Exceeding `SLOTS` or `HASHES`
//! kills the path via `kani::assume(false)` rather than wrapping — safe, but an
//! over-tight bound can leave nothing to check. If a proof passes implausibly
//! fast, add a `kani::cover` for the state you expect to reach, or assert
//! `vm.slots_touched() < SLOTS`.
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
#[cfg(kani)]
pub mod context;
#[cfg(kani)]
pub mod host;
#[cfg(kani)]
pub mod keccak;
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
#[cfg(kani)]
pub use storage::SlotStore;
