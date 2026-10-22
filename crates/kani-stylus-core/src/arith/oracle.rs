//! `U256` multiplication and division as uninterpreted functions, constrained
//! only by lemmas.
//!
//! Bit-precise `*` and `/` are where SAT-based verification gives out. Proving
//! that `a1 <= a2` implies `a1 / b <= a2 / b` for a symbolic divisor takes 0.2s
//! at 8 bits, 80s at 16, and does not finish at 32 — never mind 256. But
//! business logic rarely depends on the *bits* of a product. It depends on a
//! handful of facts about it: that it is monotone, that it is zero when an
//! operand is, that `x * y / y == x` when nothing wraps.
//!
//! So this module models exactly those facts, the way [`keccak`](crate::keccak)
//! models a hash by injectivity alone. Each `*` or `/` returns a fresh symbolic
//! value, and the solver is told the lemmas below about it — for the operation
//! alone, and against every earlier operation in the proof — and nothing else.
//! The solver then only ever compares opaque 256-bit values, which is cheap.
//! Certora and hevm do the same for Solidity; see `kb/35-arithmetic-oracle.md`.
//!
//! [`arith`](crate::arith) has the choice between this module and
//! [`exact`](crate::arith::exact), which is the one to make first. In short: a
//! counterexample from here may be spurious, so never use it for a
//! `#[kani::should_panic]` harness, which passes on *any* panic.
//!
//! # Usage
//!
//! ```ignore
//! #[kani::proof]
//! #[kani::stub(ruint::Uint::wrapping_mul, kani_stylus_core::arith::oracle::wrapping_mul_stub)]
//! #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::arith::oracle::wrapping_div_stub)]
//! #[kani::stub(ruint::Uint::overflowing_mul, kani_stylus_core::arith::oracle::overflowing_mul_stub)]
//! fn vested_is_monotone_in_time() { /* ... */ }
//! ```
//!
//! and `cargo kani -Z stubbing`. The stub path must name `ruint` directly, so
//! the contract crate needs it as an (optional) dependency. `*`, `/`, `*=`, `/=`
//! and `checked_div` all route through the first two stubs, and `checked_mul`
//! and `overflowing_mul` through the third. `%` routes through none of them and
//! stays bit-precise.
//!
//! **Stub `wrapping_mul` and `wrapping_div` together** unless the harness
//! provably never multiplies. Leaving out the mul stub never fails to compile:
//! it leaves a bit-precise 256-bit multiplier in the proof, and the result is a
//! wrong answer or no answer at all.
//!
//! # Who decides overflow
//!
//! The oracle does not compute whether a product overflowed. Each
//! multiplication records an `overflowed` flag, which is forced to `false` only
//! where that is cheap to see (the operands' bit lengths sum to at most 256),
//! and is otherwise left free. A harness assumption (`checked_mul(..).is_some()`)
//! or the contract's own branch on `checked_mul` decides it, and the lemmas that
//! need a product to fit hold relative to the flag. This lets a proof reach right
//! up to 2^256 instead of stopping at a bit-length threshold.
//!
//! The solver may set the flag on a product that would in fact fit, which
//! sends a checked contract down its overflow branch. For a safety property that
//! is conservative. For a claim that something *never* reverts, it can produce a
//! spurious failure.
//!
//! # Picking a stub: core, and decorations
//!
//! The core stubs — [`wrapping_mul_stub`], [`overflowing_mul_stub`], [`wrapping_div_stub`] —
//! carry the lemmas essentially every proof needs. A lemma that only some
//! proofs need lives on a separate, suffixed stub instead, so a proof that
//! does not need it does not pay for it.
//!
//! Today there is one decoration, `_guarded`, adding
//! [`mul_pair_guard_facts`]: an overflow guard on `a * b` carries to a product
//! `a * c` with `c <= b`. Reach for it when the contract checks one product and
//! then multiplies a related, smaller one — `initialize` guarding
//! `total * duration` while every later call computes `total * elapsed`.
//!
//! **The two mul stubs must come from the same family.** They share the record,
//! so mixing [`wrapping_mul_stub`] with [`overflowing_mul_stub_guarded`] applies the
//! extra clause to some multiplications and not others. Every lemma here is
//! true of real arithmetic, so a mismatch is never unsound — it just leaves a
//! proof with an arbitrary subset of the facts it was expecting, and that is
//! indistinguishable from the lemma being too weak.
//!
//! # What a result means
//!
//! - **A pass is sound**, as long as every lemma is true of real ruint
//!   arithmetic. Each lemma is a plain predicate below, and `cargo test`
//!   checks every one against real ruint.
//! - **A failure may be spurious.** The oracle knows only its lemmas, so it can
//!   invent a product no real multiplication gives — it does not even know that
//!   `3 * 5 == 15`. Replay a counterexample on real arithmetic before calling it
//!   a bug. If it does not reproduce, a lemma is missing; the contract is fine.
//! - **Bounded, and loudly.** The oracle records up to [`MAX_OPS`]
//!   multiplications and as many divisions, and panics past either. Silently
//!   dropping records would weaken every later lemma with no sign of why —
//!   raise the bound instead.
//! - **`U256` only.** A stub replaces the function at *every* width, so a proof
//!   that reaches `*` or `/` on any other `Uint` fails to compile rather than
//!   being silently modelled as 256-bit.

use alloy_primitives::U256;
#[cfg(kani)]
use ruint::Uint;

/// Multiplications, and separately divisions, the oracle records per proof.
/// Exceeding either panics.
pub const MAX_OPS: usize = 8;

/// One recorded operation: `r == a * b`, or `r == a / b`. `overflowed` says
/// whether the product wrapped; it is always `false` for a division.
#[derive(Clone, Copy)]
struct Op {
    a: U256,
    b: U256,
    r: U256,
    overflowed: bool,
}

// ---------------------------------------------------------------------------
// The oracle
// ---------------------------------------------------------------------------

#[cfg(kani)]
struct Oracle {
    muls: [Op; MAX_OPS],
    n_muls: usize,
    divs: [Op; MAX_OPS],
    n_divs: usize,
}

/// Kani gives each harness its own program, so this starts empty per proof.
/// Verification is single-threaded, so the unsynchronised access is sound here.
#[cfg(kani)]
static mut ORACLE: Oracle = {
    const Z: Op = Op { a: U256::ZERO, b: U256::ZERO, r: U256::ZERO, overflowed: false };
    Oracle { muls: [Z; MAX_OPS], n_muls: 0, divs: [Z; MAX_OPS], n_divs: 0 }
};

#[cfg(kani)]
fn with_oracle<R>(f: impl FnOnce(&mut Oracle) -> R) -> R {
    // SAFETY: see `ORACLE`.
    unsafe { f(&mut *core::ptr::addr_of_mut!(ORACLE)) }
}

/// Drop-in replacement for `ruint::Uint::wrapping_mul`, which `*` calls.
///
/// The signature mirrors the original, generic parameter names included — Kani
/// requires it.
#[cfg(kani)]
pub fn wrapping_mul_stub<const BITS: usize, const LIMBS: usize>(
    a: Uint<BITS, LIMBS>,
    b: Uint<BITS, LIMBS>,
) -> Uint<BITS, LIMBS> {
    from_u256(record_mul::<false>(to_u256(a), to_u256(b)).r)
}

/// [`wrapping_mul_stub`] plus [`mul_pair_guard_facts`]. Pair it with
/// [`overflowing_mul_stub_guarded`] — the two must carry the same lemma set,
/// or a proof gets an arbitrary subset of the facts it expects.
#[cfg(kani)]
pub fn wrapping_mul_stub_guarded<const BITS: usize, const LIMBS: usize>(
    a: Uint<BITS, LIMBS>,
    b: Uint<BITS, LIMBS>,
) -> Uint<BITS, LIMBS> {
    from_u256(record_mul::<true>(to_u256(a), to_u256(b)).r)
}

/// Drop-in replacement for `ruint::Uint::overflowing_mul`, which `checked_mul`
/// calls. The product is the same opaque value `wrapping_mul_stub` would give; the flag
/// is the one the lemmas are keyed on.
#[cfg(kani)]
pub fn overflowing_mul_stub<const BITS: usize, const LIMBS: usize>(
    a: Uint<BITS, LIMBS>,
    b: Uint<BITS, LIMBS>,
) -> (Uint<BITS, LIMBS>, bool) {
    let m = record_mul::<false>(to_u256(a), to_u256(b));
    (from_u256(m.r), m.overflowed)
}

/// [`overflowing_mul_stub`] plus [`mul_pair_guard_facts`], to pair with
/// [`wrapping_mul_stub_guarded`].
#[cfg(kani)]
pub fn overflowing_mul_stub_guarded<const BITS: usize, const LIMBS: usize>(
    a: Uint<BITS, LIMBS>,
    b: Uint<BITS, LIMBS>,
) -> (Uint<BITS, LIMBS>, bool) {
    let m = record_mul::<true>(to_u256(a), to_u256(b));
    (from_u256(m.r), m.overflowed)
}

/// A fresh multiplication, constrained by every lemma and recorded.
///
/// `GUARDED` is a const generic so the extra clause is compiled out entirely
/// for the undecorated stubs, rather than left as a branch in the formula.
#[cfg(kani)]
fn record_mul<const GUARDED: bool>(a: U256, b: U256) -> Op {
    let new = Op { a, b, r: U256::from_limbs(kani::any()), overflowed: kani::any() };
    kani::assume(implies(no_wrap(&new.a, &new.b), !new.overflowed));
    kani::assume(mul_facts(&new));
    with_oracle(|o| {
        let mut i = 0;
        while i < o.n_muls {
            kani::assume(mul_pair_facts(&o.muls[i], &new) && mul_pair_facts(&new, &o.muls[i]));
            if GUARDED {
                kani::assume(
                    mul_pair_guard_facts(&o.muls[i], &new)
                        && mul_pair_guard_facts(&new, &o.muls[i]),
                );
            }
            i += 1;
        }
        let mut j = 0;
        while j < o.n_divs {
            kani::assume(mul_div_facts(&new, &o.divs[j]));
            j += 1;
        }
        assert!(o.n_muls < MAX_OPS, "kani-stylus: more than MAX_OPS multiplications -- raise it");
        o.muls[o.n_muls] = new;
        o.n_muls += 1;
    });
    new
}

/// Drop-in replacement for `ruint::Uint::wrapping_div`, which `/` calls.
///
/// Division by zero panics, as it does in ruint.
#[cfg(kani)]
pub fn wrapping_div_stub<const BITS: usize, const LIMBS: usize>(
    a: Uint<BITS, LIMBS>,
    b: Uint<BITS, LIMBS>,
) -> Uint<BITS, LIMBS> {
    let new =
        Op { a: to_u256(a), b: to_u256(b), r: U256::from_limbs(kani::any()), overflowed: false };
    assert!(!eq(&new.b, &U256::ZERO), "attempt to divide by zero");
    kani::assume(div_facts(&new));
    with_oracle(|o| {
        let mut i = 0;
        while i < o.n_divs {
            kani::assume(div_pair_facts(&o.divs[i], &new) && div_pair_facts(&new, &o.divs[i]));
            i += 1;
        }
        let mut j = 0;
        while j < o.n_muls {
            kani::assume(mul_div_facts(&o.muls[j], &new));
            j += 1;
        }
        assert!(o.n_divs < MAX_OPS, "kani-stylus: more than MAX_OPS divisions -- raise it");
        o.divs[o.n_divs] = new;
        o.n_divs += 1;
    });
    from_u256(new.r)
}

#[cfg(kani)]
fn to_u256<const BITS: usize, const LIMBS: usize>(x: Uint<BITS, LIMBS>) -> U256 {
    const {
        assert!(
            BITS == 256 && LIMBS == 4,
            "kani_stylus_core::arith::oracle models U256 only, but this proof reaches `*` or `/` on another width"
        )
    };
    U256::from_limbs(core::array::from_fn(|i| x.as_limbs()[i]))
}

#[cfg(kani)]
fn from_u256<const BITS: usize, const LIMBS: usize>(x: U256) -> Uint<BITS, LIMBS> {
    Uint::from_limbs(core::array::from_fn(|i| x.as_limbs()[i]))
}

// ---------------------------------------------------------------------------
// The lemmas
//
// Each is a pure predicate over concrete operands and results, and each must
// hold for *real* ruint arithmetic -- that is the whole soundness argument, and
// the tests at the bottom check it. Divisions have `b != 0` throughout: the
// stub asserts it before any lemma is assumed.
// ---------------------------------------------------------------------------

/// `m.r == m.a * m.b`, wrapping, and `m.overflowed` says whether it wrapped.
fn mul_facts(m: &Op) -> bool {
    let (a, b, p, fits) = (&m.a, &m.b, &m.r, !m.overflowed);
    implies(eq(a, &U256::ZERO) || eq(b, &U256::ZERO), eq(p, &U256::ZERO))
        && implies(eq(a, &U256::ONE), eq(p, b))
        && implies(eq(b, &U256::ONE), eq(p, a))
        // Without wrapping, a product is at least each operand it multiplies.
        && implies(fits && !eq(b, &U256::ZERO), le(a, p))
        && implies(fits && !eq(a, &U256::ZERO), le(b, p))
}

/// Two multiplications. The stub applies this in both orders.
fn mul_pair_facts(m1: &Op, m2: &Op) -> bool {
    let same = eq(&m1.a, &m2.a) && eq(&m1.b, &m2.b);
    let swapped = eq(&m1.a, &m2.b) && eq(&m1.b, &m2.a);
    let one_operand_grows =
        (eq(&m1.a, &m2.a) && le(&m1.b, &m2.b)) || (eq(&m1.b, &m2.b) && le(&m1.a, &m2.a));
    // Functional and commutative, in the flag as well as the product. The flag
    // half is not optional: without it two calls on the same operands could
    // disagree about overflow, and the clause below would rest on nothing.
    implies(same || swapped, eq(&m1.r, &m2.r) && m1.overflowed == m2.overflowed)
        // Monotone in each operand, the other held fixed, while neither wraps.
        && implies(
            !m1.overflowed && !m2.overflowed && one_operand_grows,
            le(&m1.r, &m2.r),
        )
}

/// The `_guarded` decoration: if the larger product fits, so does the smaller.
///
/// This is what carries an overflow guard on `a * b` over to a product `a * c`
/// with `c <= b` — the shape of a contract that checks `total * duration` once
/// at construction and then multiplies `total * elapsed` on every call. Without
/// it those are unrelated records and the guard pins the flag on the wrong one.
///
/// Separate because a proof that never relates two products pays for it and
/// gets nothing back.
fn mul_pair_guard_facts(m1: &Op, m2: &Op) -> bool {
    let one_operand_grows =
        (eq(&m1.a, &m2.a) && le(&m1.b, &m2.b)) || (eq(&m1.b, &m2.b) && le(&m1.a, &m2.a));
    implies(one_operand_grows && !m2.overflowed, !m1.overflowed)
}

/// `d.r == d.a / d.b`.
fn div_facts(d: &Op) -> bool {
    let (a, b, q) = (&d.a, &d.b, &d.r);
    le(q, a)
        && implies(lt(a, b), eq(q, &U256::ZERO))
        && implies(le(b, a), !eq(q, &U256::ZERO))
        && implies(eq(b, &U256::ONE), eq(q, a))
}

/// Two divisions. The stub applies this in both orders.
fn div_pair_facts(d1: &Op, d2: &Op) -> bool {
    // Monotone in the numerator, antitone in the divisor. Together these also
    // make `/` functional: equal inputs give equal quotients.
    implies(eq(&d1.b, &d2.b) && le(&d1.a, &d2.a), le(&d1.r, &d2.r))
        && implies(eq(&d1.a, &d2.a) && le(&d1.b, &d2.b), le(&d2.r, &d1.r))
}

/// A division whose numerator is a product: `d.a == m.a * m.b`. This is the
/// `x * y / z` shape of every share, rate and vesting calculation.
fn mul_div_facts(m: &Op, d: &Op) -> bool {
    let (x, y, b, q) = (&m.a, &m.b, &d.b, &d.r);
    implies(
        eq(&d.a, &m.r) && !m.overflowed,
        // Scaling by a fraction no greater than one cannot grow a value...
        implies(le(y, b), le(q, x))
            && implies(le(x, b), le(q, y))
            // ...and scaling by exactly one leaves it alone.
            && implies(eq(y, b), eq(q, x))
            && implies(eq(x, b), eq(q, y)),
    )
}

/// `a * b` certainly fits in 256 bits: `a < 2^m` and `b < 2^n` give
/// `a * b < 2^(m + n)`. Sufficient, not necessary — a product can fit with
/// `m + n = 257` — so it only forces a multiplication's `overflowed` flag to
/// `false`. The lemmas key on the flag, not on this. The simplest case it
/// cannot see is `1 * b` for `b >= 2^255`, which never wraps but sums to 257.
fn no_wrap(a: &U256, b: &U256) -> bool {
    bit_len(a) + bit_len(b) <= 256
}

fn implies(p: bool, q: bool) -> bool {
    !p || q
}

// Limb-wise comparisons. ruint's own `==` and `cmp` go through `memcmp` and
// slice loops, which cost far more to encode than these four-limb unrollings.

fn eq(x: &U256, y: &U256) -> bool {
    let (x, y) = (x.as_limbs(), y.as_limbs());
    x[0] == y[0] && x[1] == y[1] && x[2] == y[2] && x[3] == y[3]
}

fn lt(x: &U256, y: &U256) -> bool {
    let (x, y) = (x.as_limbs(), y.as_limbs());
    let mut i = 4;
    while i > 0 {
        i -= 1;
        if x[i] != y[i] {
            return x[i] < y[i];
        }
    }
    false
}

fn le(x: &U256, y: &U256) -> bool {
    !lt(y, x)
}

fn bit_len(x: &U256) -> usize {
    let l = x.as_limbs();
    let mut i = 4;
    while i > 0 {
        i -= 1;
        if l[i] != 0 {
            return 64 * i + 64 - l[i].leading_zeros() as usize;
        }
    }
    0
}

// ---------------------------------------------------------------------------
// Soundness: every lemma holds for real ruint arithmetic.
//
// Operands are drawn so that the lemmas' premises actually fire: shared
// operands, ordered pairs, and bit lengths spread across the whole range so
// that both wrapping and non-wrapping products occur. A lemma that is false for
// real arithmetic would let Kani prove something untrue, so this is the test
// that matters most in this file.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// splitmix64: deterministic, dependency-free.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        }
        /// A value of random bit length, with small and edge values over-weighted.
        fn value(&mut self) -> U256 {
            const EDGES: [U256; 6] = [U256::ZERO, U256::ONE, U256::from_limbs([2, 0, 0, 0]),
                U256::from_limbs([u64::MAX, 0, 0, 0]), U256::from_limbs([0, 0, 1, 0]), U256::MAX];
            if self.next() % 4 == 0 {
                return EDGES[(self.next() % EDGES.len() as u64) as usize];
            }
            let v = U256::from_limbs([self.next(), self.next(), self.next(), self.next()]);
            v >> (self.next() % 256) as usize
        }
        fn nonzero(&mut self) -> U256 {
            let v = self.value();
            if v == U256::ZERO { U256::ONE } else { v }
        }
        /// `(a, MAX / a)`: the largest product with `a` that still fits. Its bit
        /// lengths often sum to 257, the region `no_wrap` cannot see into, and
        /// one more on either operand overflows.
        fn boundary_pair(&mut self) -> (U256, U256) {
            let a = self.nonzero();
            (a, U256::MAX / a)
        }
    }

    fn mul(a: U256, b: U256) -> Op {
        let (r, overflowed) = a.overflowing_mul(b);
        Op { a, b, r, overflowed }
    }
    fn div(a: U256, b: U256) -> Op {
        Op { a, b, r: a / b, overflowed: false }
    }

    const ROUNDS: usize = 20_000;

    #[test]
    fn helpers_agree_with_ruint() {
        let mut g = Rng(1);
        for _ in 0..ROUNDS {
            let (x, y) = (g.value(), g.value());
            assert_eq!(eq(&x, &y), x == y);
            assert_eq!(lt(&x, &y), x < y);
            assert_eq!(le(&x, &y), x <= y);
            assert_eq!(bit_len(&x), x.bit_len());
            assert!(!no_wrap(&x, &y) || !x.overflowing_mul(y).1, "no_wrap: {x} * {y} wraps");
        }
    }

    fn check_mul_lemmas(a: U256, b: U256, c: U256) {
        let (lo, hi) = if b <= c { (b, c) } else { (c, b) };
        let m = mul(a, b);
        assert!(mul_facts(&m), "mul_facts: {a} * {b}");
        // Pairs that share an operand, in both positions and both orders.
        for (m1, m2) in [
            (m, mul(b, a)),
            (m, mul(a, b)),
            (mul(a, lo), mul(a, hi)),
            (mul(lo, a), mul(hi, a)),
            (mul(a, b), mul(a, c)),
        ] {
            assert!(mul_pair_facts(&m1, &m2) && mul_pair_facts(&m2, &m1),
                "mul_pair_facts: {} * {} vs {} * {}", m1.a, m1.b, m2.a, m2.b);
        }
    }

    #[test]
    fn mul_lemmas_hold() {
        let mut g = Rng(2);
        for _ in 0..ROUNDS {
            check_mul_lemmas(g.value(), g.value(), g.value());
        }
    }

    /// The same, at the edge of overflow, where the lemmas now fire on the
    /// recorded flag although `no_wrap` does not hold.
    #[test]
    fn mul_lemmas_hold_at_the_overflow_boundary() {
        let mut g = Rng(5);
        let mut beyond_no_wrap = 0;
        for _ in 0..ROUNDS {
            let (a, b) = g.boundary_pair();
            beyond_no_wrap += usize::from(!no_wrap(&a, &b));
            let c = [b.saturating_add(U256::ONE), b - (b >> 1), g.value()][(g.next() % 3) as usize];
            check_mul_lemmas(a, b, c);
            check_mul_lemmas(b, a, c);
        }
        // The point of this test: products that fit but that `no_wrap` misses.
        assert!(beyond_no_wrap > ROUNDS / 4, "only {beyond_no_wrap} pairs beyond no_wrap");
    }

    /// Shrinking either operand cannot turn a product that fits into one that
    /// overflows — checked against ruint's own flag. This is what carries an
    /// overflow guard from one product to a smaller one.
    #[test]
    fn smaller_operand_cannot_overflow() {
        let mut g = Rng(6);
        for i in 0..ROUNDS {
            let (a, b2) = if i % 2 == 0 { g.boundary_pair() } else { (g.value(), g.value()) };
            let b1 = if b2 == U256::ZERO { b2 } else { g.value() % b2 };
            for (x, y1, y2) in [(a, b1, b2), (a, b1, b2.saturating_add(U256::ONE))] {
                // The predicate itself, on records built from real arithmetic:
                // this is what the stub assumes, so it is what must hold.
                assert!(
                    mul_pair_guard_facts(&mul(x, y1), &mul(x, y2)),
                    "{x} * {y1} overflows but {x} * {y2} fits"
                );
                assert!(
                    mul_pair_guard_facts(&mul(y1, x), &mul(y2, x)),
                    "{y1} * {x} overflows but {y2} * {x} fits"
                );
            }
        }
    }

    #[test]
    fn div_lemmas_hold() {
        let mut g = Rng(3);
        for _ in 0..ROUNDS {
            let (a, c, b, e) = (g.value(), g.value(), g.nonzero(), g.nonzero());
            let d = div(a, b);
            assert!(div_facts(&d), "div_facts: {a} / {b}");
            for (d1, d2) in [(d, div(c, b)), (d, div(a, e)), (d, div(a, b))] {
                assert!(div_pair_facts(&d1, &d2) && div_pair_facts(&d2, &d1),
                    "div_pair_facts: {} / {} vs {} / {}", d1.a, d1.b, d2.a, d2.b);
            }
        }
    }

    #[test]
    fn mul_div_lemmas_hold() {
        let mut g = Rng(4);
        for i in 0..ROUNDS {
            let (x, y) = if i % 2 == 0 { g.boundary_pair() } else { (g.value(), g.value()) };
            let m = mul(x, y);
            // Divisors that make each premise fire: equal to, and just above,
            // either operand -- plus an unrelated one.
            for b in [x, y, x.saturating_add(U256::ONE), y.saturating_add(U256::ONE), g.nonzero()] {
                if b == U256::ZERO {
                    continue;
                }
                assert!(mul_div_facts(&m, &div(m.r, b)), "mul_div_facts: {x} * {y} / {b}");
            }
        }
    }
}
