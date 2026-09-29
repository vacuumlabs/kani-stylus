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
//! So these stubs replace division with what division *means*:
//!
//! ```text
//!     q, r drawn freely;  r < b;  a == q*b + r
//! ```
//!
//! evaluated at 512 bits, where `q*b` cannot overflow. Those constraints pin
//! `q` and `r` uniquely, and **no unwind bound is needed anywhere**.
//!
//! # Choosing a stub
//!
//! | stub | adds | use when |
//! | --- | --- | --- |
//! | [`wrapping_div_stub`] | nothing | almost always — this is the default |
//! | [`wrapping_div_stub_memo`] | repeated identical divisions share one witness | a harness divides twice with the *same* operands |
//! | [`wrapping_div_stub_monotone`] | `a <= a' ==> q <= q'` for a shared divisor | a harness relates divisions with *different* dividends |
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
//!
//! # If the memo does not seem to be helping
//!
//! [`wrapping_div_stub_memo`] only pays off when CBMC can see that two calls
//! have identical operands. Values that round-trip through storage between the
//! calls may not compare equal structurally, in which case the lookup stays
//! symbolic and the second division builds its own multiplier anyway. To check,
//! add a counter on the hit path in [`div_rem_memo`] and
//! `kani::cover!` the expected value from your harness — a satisfied cover
//! means the memo fires. It is not instrumented permanently because the counter
//! costs formula size on every division.

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

/// How many divisions the log relates.
///
/// Purely a performance knob: truncating the log only ever drops an assumption,
/// and every assumption it adds is entailed, so a too-small value can cost
/// convergence but never soundness. Exceeding it panics rather than silently
/// dropping entries — a proof needing more should raise this.
pub const MAX_DIVS: usize = 4;

/// Widened record of the divisions performed so far: `(a, b, q, r)`.
///
/// 512 bits rather than `Uint<BITS, LIMBS>` because an item inside a generic
/// function cannot name the enclosing generic parameters. Widening is injective
/// and order-preserving, so one table serves every width — a `U64` division and
/// a `U256` division can share it harmlessly.
static mut DIV_LOG: [(Uint<512, 8>, Uint<512, 8>, Uint<512, 8>, Uint<512, 8>); MAX_DIVS] =
    [(Uint::ZERO, Uint::ZERO, Uint::ZERO, Uint::ZERO); MAX_DIVS];
static mut DIV_COUNT: usize = 0;

/// Record a division for later lookup. Shared by both wrappers below.
fn log_division(wa: Uint<512, 8>, wb: Uint<512, 8>, wq: Uint<512, 8>, wr: Uint<512, 8>) {
    unsafe {
        let log = core::ptr::addr_of_mut!(DIV_LOG);
        let cnt = core::ptr::addr_of_mut!(DIV_COUNT);
        let n = *cnt;
        if n < MAX_DIVS {
            (*log)[n] = (wa, wb, wq, wr);
            *cnt = n + 1;
        } else {
            panic!("division log full: raise MAX_DIVS");
        }
    }
}

/// Repeated divisions with identical operands share one witness, instead of
/// each building its own 512-bit multiplier.
pub fn div_rem_memo<const BITS: usize, const LIMBS: usize>(
    a: Uint<BITS, LIMBS>,
    b: Uint<BITS, LIMBS>,
) -> (Uint<BITS, LIMBS>, Uint<BITS, LIMBS>) {
    let (wa, wb) = (widen(a), widen(b));
    unsafe {
        let log = core::ptr::addr_of_mut!(DIV_LOG);
        let n = *core::ptr::addr_of_mut!(DIV_COUNT);
        let mut i = 0;
        while i < MAX_DIVS {
            if i < n {
                let (pa, pb, pq, pr) = (*log)[i];
                if ueq(&pa, &wa) && ueq(&pb, &wb) {
                    // A hit implies `b` was already checked non-zero when this
                    // entry was logged, so skipping `div_rem_spec`'s
                    // divide-by-zero assertion here is sound.
                    return (split::<BITS, LIMBS>(pq).0, split::<BITS, LIMBS>(pr).0);
                }
            }
            i += 1;
        }
    }
    let (q, r) = div_rem_spec(a, b);
    log_division(wa, wb, widen(q), widen(r));
    (q, r)
}

/// The monotonicity lemma. No memo — harnesses that need this divide with
/// *different* dividends, so a lookup would never hit and would only add
/// verification conditions.
pub fn div_rem_monotone<const BITS: usize, const LIMBS: usize>(
    a: Uint<BITS, LIMBS>,
    b: Uint<BITS, LIMBS>,
) -> (Uint<BITS, LIMBS>, Uint<BITS, LIMBS>) {
    let (wa, wb) = (widen(a), widen(b));
    let (q, r) = div_rem_spec(a, b);
    let (wq, wr) = (widen(q), widen(r));
    unsafe {
        let log = core::ptr::addr_of_mut!(DIV_LOG);
        let n = *core::ptr::addr_of_mut!(DIV_COUNT);
        let mut i = 0;
        while i < MAX_DIVS {
            if i < n {
                let (pa, pb, pq, _) = (*log)[i];
                if ueq(&pb, &wb) {
                    kani::assume(ult(&pa, &wa) || !ult(&pq, &wq)); // a  <= pa ==> q  <= pq
                    kani::assume(ult(&wa, &pa) || !ult(&wq, &pq)); // pa <= a  ==> pq <= q
                }
            }
            i += 1;
        }
    }
    log_division(wa, wb, wq, wr);
    (q, r)
}

/// Plain division. Cheapest: no log, no lemmas. The default.
pub fn wrapping_div_stub<const BITS: usize, const LIMBS: usize>(
    a: Uint<BITS, LIMBS>,
    b: Uint<BITS, LIMBS>,
) -> Uint<BITS, LIMBS> {
    div_rem_spec(a, b).0
}

/// Memoising: repeated divisions with identical operands share one witness.
pub fn wrapping_div_stub_memo<const BITS: usize, const LIMBS: usize>(
    a: Uint<BITS, LIMBS>,
    b: Uint<BITS, LIMBS>,
) -> Uint<BITS, LIMBS> {
    div_rem_memo::<BITS, LIMBS>(a, b).0
}

/// Memoising plus the monotonicity lemma. Needed only where a harness relates
/// divisions with *different* dividends.
pub fn wrapping_div_stub_monotone<const BITS: usize, const LIMBS: usize>(
    a: Uint<BITS, LIMBS>,
    b: Uint<BITS, LIMBS>,
) -> Uint<BITS, LIMBS> {
    div_rem_monotone::<BITS, LIMBS>(a, b).0
}
