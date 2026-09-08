//! Worked examples for `kani-stylus-core`.
//!
//! Two contracts, verified:
//!
//! - [`counter`] — the stock `cargo stylus new` template. Storage round-trips,
//!   and a real silent-overflow bug that Kani finds with a concrete witness.
//! - [`vault`] — owner-gated methods over a symbolic caller, plus mappings,
//!   which exercise the keccak oracle.
//!
//! ```bash
//! cargo kani -Z stubbing --output-format terse          # everything
//! cargo kani --harness counter::proofs::set_then_get_roundtrips
//! ```
//!
//! `-Z stubbing` is needed by the mapping proofs; harnesses without mappings
//! run without it.

extern crate alloc;

pub mod counter;
pub mod vault;
