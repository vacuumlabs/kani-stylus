//! `U256` division, modelled by its specification rather than its algorithm.
//!
//! A single `U256` division is enough to make a proof hopeless. `ruint`'s
//! `div_rem` works over `&[u64]` slices whose lengths come from the divisor's
//! leading zeros, so CBMC cannot bound its loops — symbolic execution runs away
//! before any solver is involved, measured at no formula produced in 30
//! minutes. Loop bounds do not fix it: `unwind(5)` truncates the `memcmp` that
//! `Address` comparison needs at 33, `unwind(33)` exhausts 23 GiB because the
//! loops nest, and per-loop `--unwindset` is whack-a-mole.
//!
//! So this stub replaces division with what division *means*:
//!
//! ```text
//!     q, r drawn freely;  r < b;  a == q*b + r
//! ```
//!
//! evaluated at 512 bits, where `q*b` cannot overflow. Those constraints pin
//! `q` and `r` uniquely, and **no unwind bound is needed anywhere**. What they
//! do not give is any *relational* fact about two different divisions — see
//! [`arith`](crate::arith) for when that means you want the oracle instead.
//!
//! ```ignore
//! #[kani::proof]
//! #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::arith::exact::wrapping_div_stub)]
//! fn my_property() { /* ... */ }
//! ```
//!
//! `/` lowers to `wrapping_div` and `%` to `wrapping_rem` — two separate
//! `impl_bin_op!` lines in ruint's `div.rs` — so a contract using both needs
//! both stubs. Do **not** stub `div_rem` instead: it is `#[inline(always)]`, so
//! Kani silently fails to replace it and the proof hangs exactly as if no stub
//! were applied. Forgetting a stub does not error, it hangs; `trim_end_zeros`
//! or `cmp` climbing in the log is the signature.

use ruint::Uint;

/// Limb-wise equality. Replaces `==` on `Uint`, which lowers to `memcmp` and
/// then needs an unwind bound of 33. The loop here is bounded by `LIMBS`, a
/// const generic, so CBMC unrolls it exactly.
pub fn ueq<const BITS: usize, const LIMBS: usize>(
    x: &Uint<BITS, LIMBS>,
    y: &Uint<BITS, LIMBS>,
) -> bool {
    let (xa, ya) = (x.as_limbs(), y.as_limbs());
    let mut i = 0;
    let mut eq = true;
    while i < LIMBS {
        if xa[i] != ya[i] {
            eq = false;
        }
        i += 1;
    }
    eq
}

/// Limb-wise `<`. Walks from the most significant limb down, which assumes
/// `Uint::as_limbs()` is least-significant-first — ruint's documented layout,
/// and a limb *order* assumption rather than one about byte endianness.
/// Discharge it with a proof that `ueq`/`ult` agree with ruint's own operators.
pub fn ult<const BITS: usize, const LIMBS: usize>(
    x: &Uint<BITS, LIMBS>,
    y: &Uint<BITS, LIMBS>,
) -> bool {
    let (xa, ya) = (x.as_limbs(), y.as_limbs());
    let mut i = LIMBS;
    while i > 0 {
        i -= 1;
        if xa[i] != ya[i] {
            return xa[i] < ya[i];
        }
    }
    false
}

/// Zero-pad any `Uint` up to 512 bits. Loop bound is `LIMBS`, a const generic,
/// so CBMC unrolls it exactly — no slice length is ever involved.
///
/// 512 bits is not arbitrary: `ADDMUL_N_SMALL_LIMIT` is 8 and `Uint<512, 8>` is
/// exactly 8 limbs, so `U512 * U512` takes ruint's fast, statically bounded
/// `addmul_n_small` path. One limb more and it would fall back to the
/// slice-based `addmul` that caused the whole problem.
fn widen<const BITS: usize, const LIMBS: usize>(x: Uint<BITS, LIMBS>) -> Uint<512, 8> {
    const { assert!(LIMBS <= 8, "widen: only Uints up to 512 bits fit in a Uint<512, 8>") };
    let mut l = [0u64; 8];
    let xl = x.as_limbs();
    let mut i = 0;
    while i < LIMBS {
        l[i] = xl[i];
        i += 1;
    }
    Uint::from_limbs(l)
}

/// A symbolic `Uint` guaranteed to be a *valid* value of its width.
///
/// `from_limbs` asserts that the top limb fits in `BITS`, and there is no
/// masking constructor. An unconstrained draw trips that for widths which are
/// not a multiple of 64 (`uint96`, `uint160`); the assumption below is free at
/// 256.
fn any_uint<const BITS: usize, const LIMBS: usize>() -> Uint<BITS, LIMBS> {
    let limbs: [u64; LIMBS] = kani::any();
    kani::assume(limbs[LIMBS - 1] <= Uint::<BITS, LIMBS>::MASK);
    Uint::from_limbs(limbs)
}

/// What division *means*: a nondeterministic witness constrained to be the
/// unique quotient and remainder. Both stubs below are thin wrappers over it.
fn div_rem_spec<const BITS: usize, const LIMBS: usize>(
    a: Uint<BITS, LIMBS>,
    b: Uint<BITS, LIMBS>,
) -> (Uint<BITS, LIMBS>, Uint<BITS, LIMBS>) {
    // Soundness: `q * b` must not overflow the widened type, so 2*BITS <= 512.
    const {
        assert!(
            LIMBS >= 1 && BITS <= 256,
            "kani-stylus: the division stub is sound only for Uints of 1..=256 bits"
        )
    };

    assert!(!ueq(&b, &Uint::ZERO), "attempt to divide by zero");

    let q = any_uint::<BITS, LIMBS>();
    let r = any_uint::<BITS, LIMBS>();

    kani::assume(ult(&r, &b));
    // a == q*b + r, evaluated at 512 bits where the product cannot overflow.
    kani::assume(ueq(&(widen(q) * widen(b) + widen(r)), &widen(a)));

    (q, r)
}

/// Stub for `ruint::Uint::wrapping_div`, which is what `/` lowers to.
pub fn wrapping_div_stub<const BITS: usize, const LIMBS: usize>(
    a: Uint<BITS, LIMBS>,
    b: Uint<BITS, LIMBS>,
) -> Uint<BITS, LIMBS> {
    div_rem_spec(a, b).0
}

/// Stub for `ruint::Uint::wrapping_rem`, which is what `%` lowers to.
pub fn wrapping_rem_stub<const BITS: usize, const LIMBS: usize>(
    a: Uint<BITS, LIMBS>,
    b: Uint<BITS, LIMBS>,
) -> Uint<BITS, LIMBS> {
    div_rem_spec(a, b).1
}

#[cfg(kani)]
mod proofs {
    use super::{ueq, ult};
    use crate::any_u256;

    /// Discharges the limb-order assumption `ult` rests on. A Kani harness
    /// rather than a `cargo test`, because this has to hold for *every* pair,
    /// not a sample of them.
    #[kani::proof]
    fn comparison_helpers_agree_with_ruint() {
        let x = any_u256();
        let y = any_u256();
        assert_eq!(ueq(&x, &y), x == y);
        assert_eq!(ult(&x, &y), x < y);
    }
}

