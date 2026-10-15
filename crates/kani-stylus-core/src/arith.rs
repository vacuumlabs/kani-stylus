//! `U256` division, modelled by its specification rather than its algorithm.
//!
//! Nothing in a Stylus contract divides more than a handful of times, but a
//! single `U256` division is enough to make a proof hopeless. `ruint`'s
//! `div_rem` works over `&[u64]` slices whose lengths come from the divisor's
//! leading zeros, so CBMC cannot bound its loops: symbolic execution runs away
//! before any solver is involved. Measured, a bare `U256` division with no
//! unwind bound produced **no formula at all in 30 minutes**, sitting at
//! iteration 2299 of `ruint::algorithms::cmp`.
//!
//! Loop bounds do not fix it. `#[kani::unwind(5)]` truncates `memcmp`, which
//! `Address` comparison needs at 33; `#[kani::unwind(33)]` exhausts 23 GiB,
//! because the division's loops nest. Per-loop `--unwindset` is whack-a-mole —
//! bounding `cmp` merely moves the problem to `slice::fill`.
//!
//! So this stub replaces division with what division *means*:
//!
//! ```text
//!     q, r drawn freely;  r < b;  a == q*b + r
//! ```
//!
//! evaluated at 512 bits, where `q*b` cannot overflow. Those constraints pin
//! `q` and `r` uniquely, and **no unwind bound is needed anywhere**.
//!
//! [`wrapping_div_stub`] supplies no *relational* fact about different
//! dividends. A property that needs one — monotonicity, `x * y / z <= x` —
//! belongs on [`arith_oracle`](crate::arith_oracle) instead, which drops
//! exactness to buy those facts cheaply. This module keeps division exact, so
//! every counterexample it produces is real; that is what it is for.
//!
//! ```ignore
//! #[kani::proof]
//! #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::wrapping_div_stub)]
//! fn my_property() { /* ... */ }
//! ```
//!
//! `/` and `%` both lower to `wrapping_div` — `impl_bin_op!(Div, div,
//! DivAssign, div_assign, wrapping_div)`. Do **not** try to stub `div_rem`
//! instead: it is `#[inline(always)]`, so Kani silently fails to replace it and
//! the proof hangs exactly as if no stub were applied.
//!
//! Stubs are per-harness and forgetting one does not error, it hangs. If a
//! harness runs away with `trim_end_zeros` or `cmp` climbing in the log, a stub
//! attribute is missing — that is the signature.
//!
//! # Why the monotonicity lemma is sound
//!
//! For two divisions with the same divisor, `a <= a' ==> q <= q'`. If `q > q'`
//! then `q >= q'+1`, so `a = q*b + r >= (q'+1)*b = q'*b + b > q'*b + r' = a'`,
//! using `r' < b`. The contrapositive is the claim.
//!
//! It is therefore *entailed* by the constraints above, which already pin `q`
//! uniquely — it removes no models and cannot make a proof vacuous. What it
//! buys is an inference CBMC cannot reach unaided: the same statement at
//! primitive `u64` width, with no contract and no stub, is ten verification
//! conditions and does not converge in five minutes.
//!
//! Note the direction. These are two implications, **not** a biconditional:
//! `q <= q'` does not imply `a <= a'` (take `a = 3`, `a' = 2`, `b = 2`).

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
pub fn widen<const BITS: usize, const LIMBS: usize>(x: Uint<BITS, LIMBS>) -> Uint<512, 8> {
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

/// Narrow a 512-bit value to `(low BITS bits, whether anything was lost)`.
pub fn split<const BITS: usize, const LIMBS: usize>(
    w: Uint<512, 8>,
) -> (Uint<BITS, LIMBS>, bool) {
    let wl = w.as_limbs();
    let mut overflow = false;

    let mut i = LIMBS;
    while i < 8 {
        if wl[i] != 0 {
            overflow = true;
        }
        i += 1;
    }

    let mut l = [0u64; LIMBS];
    let mut j = 0;
    while j < LIMBS {
        l[j] = wl[j];
        j += 1;
    }

    let mask = Uint::<BITS, LIMBS>::MASK;
    if l[LIMBS - 1] > mask {
        overflow = true;
        l[LIMBS - 1] &= mask;
    }

    (Uint::from_limbs(l), overflow)
}

/// A symbolic `Uint` guaranteed to be a *valid* value of its width.
///
/// `from_limbs` asserts that the top limb fits in `BITS`, and there is no
/// masking constructor. An unconstrained draw trips that for widths which are
/// not a multiple of 64 (`uint96`, `uint160`); the assumption below is free at
/// 256.
pub fn any_uint<const BITS: usize, const LIMBS: usize>() -> Uint<BITS, LIMBS> {
    let limbs: [u64; LIMBS] = kani::any();
    kani::assume(limbs[LIMBS - 1] <= Uint::<BITS, LIMBS>::MASK);
    Uint::from_limbs(limbs)
}

/// What division *means*: a nondeterministic witness constrained to be the
/// unique quotient and remainder. No log, no lemmas. Every stub here builds on
/// it.
pub fn div_rem_spec<const BITS: usize, const LIMBS: usize>(
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

/// Stub for `ruint::Uint::wrapping_div`.
pub fn wrapping_div_stub<const BITS: usize, const LIMBS: usize>(
    a: Uint<BITS, LIMBS>,
    b: Uint<BITS, LIMBS>,
) -> Uint<BITS, LIMBS> {
    div_rem_spec(a, b).0
}

