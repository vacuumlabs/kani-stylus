//! `U256` arithmetic for proofs, in two modes — pick one per harness.
//!
//! A single symbolic `U256` division is enough to make a proof diverge, and a
//! symbolic 256-bit multiplier is enough to make it unprovable in practice. Both
//! modules below replace `*` and `/` with something a solver can handle. They
//! differ in what they give up to do it, and that difference decides which one a
//! harness wants.
//!
//! | | [`exact`] | [`oracle`] |
//! | --- | --- | --- |
//! | models division by | its specification: `a == q*b + r`, `r < b` | lemmas about `*` and `/` |
//! | a counterexample is | **real** | possibly spurious |
//! | a passing proof is | sound | sound |
//! | relational facts (`x*y/z <= x`, monotonicity) | none | its whole point |
//! | cost | a 512-bit multiplier per division | no multiplier at all |
//!
//! **Use [`exact`] where a counterexample must be believed** — every
//! `#[kani::should_panic]` harness, and anything demonstrating a bug. A
//! fabricated quotient would satisfy such a harness for the wrong reason.
//!
//! **Use [`oracle`] to prove a property** that rests on how `*` and `/` relate
//! to each other. Those facts are what the exact mode cannot supply at any
//! price: it pins each quotient uniquely but says nothing about two of them, so
//! monotonicity or `total * e / d <= total` sits behind a 512-bit multiplier the
//! solver has to reason through.
//!
//! Both modes stub the same ruint functions, so **a stub is named after the
//! function it replaces** and the module says which mode it is:
//! `exact::wrapping_div_stub` and `oracle::wrapping_div_stub` are both
//! replacements for `ruint::Uint::wrapping_div`. Spelling the mode in the path
//! keeps it visible at every use site:
//!
//! ```ignore
//! #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::arith::exact::wrapping_div_stub)]
//! ```
//!
//! The two halves of each attribute match, which is the point — pointing
//! `wrapping_div` at a multiplication stub is then visible on the line rather
//! than at three in the morning. Getting the stub set wrong is otherwise silent:
//! it never fails to compile, it produces a wrong answer or no answer.
//!
//! Mixing modes within one harness is sound but rarely what you want, with one
//! exception that earns it: a `should_panic` harness whose defect is a rounding
//! loss needs [`exact`] division, yet may still reach `checked_mul` through a
//! contract's own overflow guard — and that can take `oracle::overflowing_mul_stub`,
//! because the harness's `assume` pins the flag rather than fabricating it.

#[cfg(kani)]
pub mod exact;

// The lemmas are plain predicates, and `cargo test` checks them against real
// ruint arithmetic — that check is the module's soundness argument, so it has
// to build without Kani. Only its stubs are `cfg(kani)`.
#[cfg(any(kani, test))]
pub mod oracle;
