//! Features the stock counter can't demonstrate.
//!
//! The primary example is [`stylus-samples/counter`](../../../stylus-samples/counter),
//! a real `cargo stylus new` project with proofs added in place — start there
//! to see how kani-stylus drops into an ordinary Stylus setup.
//!
//! This crate covers what that contract has no occasion for:
//!
//! - **access control** over a symbolic caller (`msg_sender`);
//! - **mappings**, whose slots are keccak-derived and so exercise the hash
//!   oracle in `kani_stylus_core::keccak`.
//!
//! It is proofs-only and never deployed, so unlike a real contract it enables
//! `stylus-test` unconditionally rather than behind a feature.
//!
//! ```bash
//! cargo kani -Z stubbing --output-format terse
//! ```
//!
//! `-Z stubbing` is required: the mapping proofs stub `stylus_sdk::crypto::keccak`.

extern crate alloc;

pub mod vault;
