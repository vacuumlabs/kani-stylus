//! Tiny pure-Rust harnesses, chosen so the formula CBMC generates is small
//! enough to read line by line. No Stylus, no `U256`, no allocation.
//!
//! Dump any harness's formula with `./dump.sh <harness>`; see README.md.

#![cfg(kani)]

// ---------------------------------------------------------------------------
// 1. The Rosetta stone: one symbolic value.
// ---------------------------------------------------------------------------

/// The smallest harness that still has a free variable and a property.
/// Its formula is ~88 lines and is walked through in README.md.
#[kani::proof]
fn h1_nondet() {
    let a: u8 = kani::any();
    kani::assume(a < 10);
    assert!(a < 10);
}

// ---------------------------------------------------------------------------
// 2. Where the automatic overflow check comes from.
// ---------------------------------------------------------------------------

/// `a + b` on `u8` is a *checked* add: rustc is invoked with
/// `-C overflow-checks=on`, so it emits a panic branch, which becomes a second
/// property in the formula. Look for `bvadd` and the `add_overflow` property.
///
/// Drop the `assume` and this fails with a concrete counterexample — that is
/// the single most useful thing to try in this file.
#[kani::proof]
fn h2_add_guarded() {
    let a: u8 = kani::any();
    let b: u8 = kani::any();
    kani::assume(a <= 100 && b <= 100);
    let sum = a + b;
    assert!(sum >= a);
}

// ---------------------------------------------------------------------------
// 3. Stubbing, at the formula level. Needs `-Z stubbing`.
// ---------------------------------------------------------------------------

/// Stands in for "something we do not want in the formula". The body is one
/// `xor` so that its footprint is a single, greppable SMT operator.
fn mask(x: u8) -> u8 {
    x ^ 0x5a
}

/// A stub that is genuinely equivalent *for this property*: masking twice is
/// the identity either way. Sound, and the `bvxor` vanishes from the formula.
fn mask_identity(x: u8) -> u8 {
    x
}

/// A stub that throws the body away entirely: every call returns a fresh
/// unconstrained value. This is what "over-approximation" means concretely.
fn mask_any(_x: u8) -> u8 {
    kani::any()
}

/// Baseline: the real body. Two `bvxor` in the formula.
#[kani::proof]
fn h3_stub_none() {
    let x: u8 = kani::any();
    assert!(mask(mask(x)) == x);
}

/// Same property, `mask` replaced by an equivalent. Still verifies, and the
/// `bvxor` is gone: `mask` is no longer part of the formula at all.
#[kani::proof]
#[kani::stub(mask, mask_identity)]
fn h3_stub_equivalent() {
    let x: u8 = kani::any();
    assert!(mask(mask(x)) == x);
}

/// Same property, `mask` replaced by `kani::any()`. Now **fails** — not
/// because the code is wrong, but because the stub is weaker than the code it
/// replaced. Each call became its own free variable, so the solver is free to
/// return two unrelated values. A stub is a proof obligation you took on
/// yourself; this harness is what forgetting that looks like.
#[kani::proof]
#[kani::should_panic]
#[kani::stub(mask, mask_any)]
fn h3_stub_any_overapproximates() {
    let x: u8 = kani::any();
    assert!(mask(mask(x)) == x);
}
