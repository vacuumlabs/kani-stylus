//! A linear vesting schedule, verified with Kani.
//!
//! This is an ordinary Stylus contract — `cargo stylus new vesting`, with the
//! contract body written and proofs added in place. It covers three things
//! neither the [counter](../../counter) nor the [vault](../../vault) has any
//! occasion for:
//!
//! - **time** — `releasable` reads `block_timestamp`, which `SymbolicVM` has
//!   drawn symbolically since day one without a single harness ever looking at
//!   it. A proof over the clock covers every instant at once;
//! - **integer division**, and the rounding that comes with it — nothing else
//!   in this repo divides;
//! - **monotonicity**, a property shape the existing suites don't have: the
//!   vested amount must never go backwards as time moves forward.
//!
//! ```bash
//! cargo test                                  # ordinary unit tests
//! cargo kani --features proofs -Z stubbing    # the proofs
//! ```
//!
//! `-Z stubbing` is required, though for a different reason than the vault's.
//! There are no mappings here, so nothing hashes through
//! `stylus_sdk::crypto::keccak` — but every harness that reaches the vesting
//! formula replaces `ruint::Uint::wrapping_div`, because a single symbolic
//! `U256` division is otherwise enough to make a proof diverge. Two families of
//! stub do it: [`kani_stylus_core::arith`] models division exactly, and is used
//! wherever a counterexample must be real; [`kani_stylus_core::arith_oracle`]
//! replaces `*` and `/` with lemma-constrained unknowns, and is what makes the
//! monotonicity and bound proofs converge.
//!
//! The model is OpenZeppelin's `VestingWallet` shape — **one deployed instance
//! is one entitlement**, not a registry of many.
//!
//! Like the vault's `balances`, this is **accounting only**: `release`
//! increments a counter and nothing is transferred. Holding ETH would need
//! `balance()` plus outbound value transfer, and vesting an ERC-20 would need
//! cross-contract calls — all `unimplemented!()` in the symbolic host today.
//!
//! Note: this code is illustrative and has not been audited. It contains
//! **five deliberate defects**, each marked `DELIBERATE DEFECT n` at the site:
//!
//! | # | where | what | harness |
//! | --- | --- | --- | --- |
//! | 1 | `schedule` | `start + duration` overflows `u64`, so a far-future schedule reports everything vested at once | Kani finds it unaided — `u64` is primitive |
//! | 2 | `schedule` | `total * (t - start)` wraps silently; `ruint` defines `*` as `wrapping_mul` and Kani's overflow checks cover primitives only | `vested_unchecked_overflows` |
//! | 3 | `release_drifting` | accrue-per-call truncates on every claim, so the shortfall grows with how often the beneficiary claims | `drifting_release_loses_value` |
//! | 4 | `initialize` | the re-init guard is "beneficiary is non-zero", so initializing *to* the zero address never latches | `zero_beneficiary_never_latches` |
//! | 5 | `releasable` | bare `-` wraps when `released` exceeds what has vested | none yet |
//!
//! Defects 1 and 2 are the same bug in `u64` and in `U256`, and the contrast is
//! the point: one Kani finds on its own, the other is invisible unless a proof
//! asserts it.

#![cfg_attr(not(any(test, feature = "export-abi")), no_main)]
extern crate alloc;

use alloc::vec::Vec;

use stylus_sdk::{
    alloy_primitives::{Address, U256, U64},
    prelude::*,
};

sol_storage! {
    #[entrypoint]
    pub struct Vesting {
        address beneficiary;
        uint256 total;
        uint256 released;         // release()
        uint256 released_naive;   // release_drifting()
        uint64 start;
        uint64 duration;
        uint64 last_claim;        // release_drifting()
    }
}

#[public]
impl Vesting {
    pub fn beneficiary(&self) -> Address {
        self.beneficiary.get()
    }

    pub fn total(&self) -> U256 {
        self.total.get()
    }

    /// Released so far by `release`.
    pub fn released(&self) -> U256 {
        self.released.get()
    }

    /// Released so far by `release_drifting`. Tracked separately so the two
    /// schemes can be compared side by side from the same pre-state.
    pub fn released_naive(&self) -> U256 {
        self.released_naive.get()
    }

    // The three `uint64` fields are `StorageU64`, so `get()` hands back an
    // alloy `U64` rather than a Rust `u64`. These getters convert once, at the
    // storage boundary, and every schedule calculation below then runs on
    // primitive `u64`.
    //
    // That is deliberate, not cosmetic. `U64` is a ruint type, and ruint
    // defines `+` as `wrapping_add` — arithmetic on it would wrap in silence
    // and Kani would not see it. On `u64`, Kani's automatic overflow checks
    // apply, which is what makes the `start + duration` defect below findable
    // without asserting anything.

    pub fn start(&self) -> u64 {
        self.start.get().to::<u64>()
    }

    pub fn duration(&self) -> u64 {
        self.duration.get().to::<u64>()
    }

    pub fn last_claim(&self) -> u64 {
        self.last_claim.get().to::<u64>()
    }

    /// One-shot setup, in the style of the vault's `claim_ownership`.
    ///
    /// Rejecting `duration == 0` is what keeps `release_drifting`'s division
    /// safe in normal operation.
    ///
    /// DELIBERATE DEFECT 4 — do not "fix".
    ///
    /// The same gap the vault's `claim_ownership` has: the already-initialized
    /// guard is "the beneficiary is non-zero", so initializing *to* the zero
    /// address leaves it unlatched and lets this be run again. Left as-is to
    /// match the vault rather than quietly diverge from it.
    ///
    /// Demonstrated by `zero_beneficiary_never_latches`.
    pub fn initialize(
        &mut self,
        beneficiary: Address,
        total: U256,
        start: u64,
        duration: u64,
    ) -> Result<(), Vec<u8>> {
        if self.beneficiary.get() != Address::ZERO {
            return Err(b"already initialized".to_vec());
        }
        if duration == 0 {
            return Err(b"zero duration".to_vec());
        }

        // Reject a schedule whose arithmetic cannot be carried out, rather than
        // leaving `vested_amount` to wrap on every later call. Both checks are
        // about the *schedule*, so construction is where they belong.
        //
        // The first is DELIBERATE DEFECT 1's precondition: `start + duration`
        // must not overflow `u64`, or the schedule's end wraps below its start.
        if start.checked_add(duration).is_none() {
            return Err(b"schedule end overflows".to_vec());
        }
        // The second is DELIBERATE DEFECT 2's, and it covers *every* later `t`.
        // `vested_amount` multiplies `total * (t - start)` only in its third
        // branch, where `t - start < duration`, so `total * (t - start)` is at
        // most `total * duration`. Bounding the largest case bounds them all,
        // and it needs checking once here instead of on every call.
        if total.checked_mul(U256::from(duration)).is_none() {
            return Err(b"total too large for this duration".to_vec());
        }

        self.beneficiary.set(beneficiary);
        self.total.set(total);
        self.start.set(U64::from(start));
        self.duration.set(U64::from(duration));
        self.last_claim.set(U64::from(start));
        Ok(())
    }

    /// How much has vested by time `t` — a pure function of its argument, so a
    /// proof can quantify over the clock without the host being involved.
    ///
    /// Branch order matters: branch 2 catches `duration == 0` before branch 3
    /// can divide by it. Reaching branch 2 means `t >= start`, so with a zero
    /// duration `t >= start + 0` holds and the division is unreachable.
    pub fn vested_amount(&self, t: u64) -> U256 {
        let start = self.start();
        if t < start {
            return U256::ZERO;
        }

        let duration = self.duration();
        let total = self.total.get();

        // Both checks panic rather than returning an `Err`. `initialize`
        // rejects any schedule that could reach them, so arriving here means
        // the contract is in a state it should not be able to be in — a bug,
        // not a caller error, and reverting loudly is the right answer. It also
        // keeps this method, `releasable` and `release` returning plain values
        // instead of threading a `Result` through all three for a case that
        // cannot arise.
        //
        // `vested_amount_unchecked` is this function without them, and is where
        // DELIBERATE DEFECTS 1 and 2 live.
        let end = start.checked_add(duration).expect("schedule end overflows");
        if t >= end {
            return total;
        }

        // `t - start` is below `duration` here, so this product is at most
        // `total * duration` -- exactly what `initialize` checked.
        let scaled = total
            .checked_mul(U256::from(t - start))
            .expect("total too large for this schedule");
        scaled / U256::from(duration)
    }

    pub fn vested_amount_unchecked(&self, t: u64) -> U256 {
        let start = self.start();
        if t < start {
            return U256::ZERO;
        }

        let duration = self.duration();
        let total = self.total.get();
        // DELIBERATE DEFECT 1 — do not "fix".
        //
        // `start + duration` overflows `u64` for a far-future schedule, and
        // the wrapped sum lands below `t`, so the contract reports everything
        // vested from the moment it is set up.
        //
        // This one Kani finds *on its own*: `u64` is a primitive, and
        // arithmetic overflow checks are on by default. Contrast defect 2,
        // four lines down, which is the same bug in `U256` and is invisible
        // unless a proof asserts it. Having one of each is the point — it puts
        // the distinction in front of anyone reading the suite.
        if t >= start + duration {
            return total;
        }

        // DELIBERATE DEFECT 2 — do not "fix".
        //
        // `total * (t - start)` wraps silently. `alloy`/`ruint` define `*` as
        // `wrapping_mul`, so for a large `total` this can report *more* than
        // `total` vested, or wrap back to near zero. Kani's overflow checks
        // cover primitive integers only and will not flag it; a harness has to
        // assert the bound explicitly.
        total * U256::from(t - start) / U256::from(duration)
    }

    /// Vested but not yet released. The only place the host clock is read,
    /// which is what keeps `vested_amount` pure in its argument.
    pub fn releasable(&self) -> U256 {
        // DELIBERATE DEFECT 5 — do not "fix".
        //
        // Bare `-`, and so a silent wrap if `released` ever exceeds what has
        // vested. That cannot happen through this contract's own methods —
        // `release` only ever adds what `releasable` just returned — but it is
        // reachable from hand-seeded pre-state, which is exactly how a proof
        // sets things up. OpenZeppelin's version leans on the same invariant,
        // so the bare subtraction is the realistic thing to model.
        //
        // No harness yet. Every release-side proof silently depends on
        // `released <= vested(now)` holding; that assumption is currently
        // neither stated nor demonstrated.
        self.vested_amount(self.vm().block_timestamp()) - self.released.get()
    }

    /// The correct form: recompute from the schedule, then subtract what has
    /// already gone out.
    ///
    /// `released` ends up at exactly `vested_amount(now)` after any sequence
    /// of calls, however often they came — there is no per-call remainder to
    /// accumulate, because nothing is ever divided twice.
    pub fn release(&mut self) -> U256 {
        let amount = self.releasable();
        self.released.set(self.released.get() + amount);
        amount
    }

    /// DELIBERATE DEFECT 3 — do not "fix". The headline finding of this
    /// example.
    ///
    /// The accrue-per-call form: work out what has accrued since the last
    /// claim and add it on. It looks equivalent to `release` and is not.
    ///
    /// `total * elapsed / duration` truncates on **every** call, so the
    /// shortfall grows with how often the beneficiary claims. Claim once and
    /// the loss is a single rounding error; claim every block and it compounds
    /// into real money. A beneficiary who claims frequently is punished for
    /// it, which is the sort of frequency-dependence that shows up in real
    /// contracts and is invisible to a test that claims twice.
    ///
    /// Written as `total * elapsed / duration` rather than with a precomputed
    /// `rate = total / duration` on purpose: a precomputed rate loses a fixed
    /// remainder no matter the call pattern, which is a duller bug.
    ///
    /// Two further sharp edges, both unreachable through `initialize` but not
    /// from seeded pre-state: the division is unguarded, so `duration == 0`
    /// panics, and `now - last_claim` underflows if the clock ever runs
    /// backwards.
    pub fn release_drifting(&mut self) -> U256 {
        let now = self.vm().block_timestamp();
        let elapsed = now - self.last_claim();

        let amount = self.total.get() * U256::from(elapsed) / U256::from(self.duration());

        self.released_naive.set(self.released_naive.get() + amount);
        self.last_claim.set(U64::from(now));
        amount
    }
}

/// Ordinary unit tests: one concrete scenario each. Compare with the proofs to
/// come, which will cover every instant and every amount at once.
#[cfg(test)]
mod test {
    use super::*;
    use stylus_sdk::testing::*;

    const TOTAL: u64 = 1000;
    const START: u64 = 100;
    const DURATION: u64 = 100;

    fn setup() -> (TestVM, Vesting) {
        let vm = TestVM::default();
        let mut vesting = Vesting::from(&vm);
        vesting
            .initialize(
                Address::from([1u8; 20]),
                U256::from(TOTAL),
                START,
                DURATION,
            )
            .unwrap();
        (vm, vesting)
    }

    #[test]
    fn test_schedule() {
        let (_vm, vesting) = setup();

        // Nothing before the cliff.
        assert_eq!(U256::ZERO, vesting.vested_amount(0));
        assert_eq!(U256::ZERO, vesting.vested_amount(START - 1));
        // Linear in between.
        assert_eq!(U256::ZERO, vesting.vested_amount(START));
        assert_eq!(U256::from(500), vesting.vested_amount(START + DURATION / 2));
        // Everything at the end, and nothing more after it.
        assert_eq!(U256::from(TOTAL), vesting.vested_amount(START + DURATION));
        assert_eq!(
            U256::from(TOTAL),
            vesting.vested_amount(START + DURATION * 10)
        );
    }

    #[test]
    fn test_release() {
        let (vm, mut vesting) = setup();

        // Halfway: half the total is releasable, and taking it leaves nothing.
        vm.set_block_timestamp(START + DURATION / 2);
        assert_eq!(U256::from(500), vesting.releasable());
        assert_eq!(U256::from(500), vesting.release());
        assert_eq!(U256::from(500), vesting.released());
        assert_eq!(U256::ZERO, vesting.releasable());

        // At the end: the rest, and `released` lands on `total` exactly.
        vm.set_block_timestamp(START + DURATION);
        assert_eq!(U256::from(500), vesting.release());
        assert_eq!(U256::from(TOTAL), vesting.released());

        // Past the end there is nothing left to take.
        vm.set_block_timestamp(START + DURATION * 2);
        assert_eq!(U256::ZERO, vesting.release());
        assert_eq!(U256::from(TOTAL), vesting.released());
    }

    #[test]
    fn test_cannot_reinitialize() {
        let (_vm, mut vesting) = setup();

        assert!(vesting
            .initialize(Address::from([2u8; 20]), U256::from(1), 0, 1)
            .is_err());

        // The original schedule survived the attempt.
        assert_eq!(Address::from([1u8; 20]), vesting.beneficiary());
        assert_eq!(U256::from(TOTAL), vesting.total());
        assert_eq!(START, vesting.start());
    }
}

#[cfg(all(kani, not(feature = "proofs")))]
compile_error!("proofs need the `proofs` feature: cargo kani --features proofs");




#[cfg(kani)]
mod proofs {
    use super::*;
    // The division stubs are named by full path in the `#[kani::stub(..)]`
    // attributes, so they need no import here.
    use kani_stylus_core::{any_u256, ueq, ult, SymbolicVM, SymbolicVm};

    use kani_stylus_core::Context;

    use ruint::Uint;

    fn div_stub_always_zero<const BITS: usize, const LIMBS: usize>(
        _a: Uint<BITS, LIMBS>, _b: Uint<BITS, LIMBS>,
    ) -> Uint<BITS, LIMBS> { Uint::ZERO }

    #[kani::proof]
    fn comparison_helpers_agree_with_ruint() {
        let x = any_u256();
        let y = any_u256();
        assert_eq!(ueq(&x, &y), x == y);
        assert_eq!(ult(&x, &y), x < y);
    }

    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_div, crate::proofs::div_stub_always_zero)]
    fn stub_takes_effect() {
        let a = core::hint::black_box(U256::from(10u64));
        let b = core::hint::black_box(U256::from(2u64));
        assert_eq!(a / b, U256::ZERO);   // true ONLY if the stub replaced the division
    }

    #[kani::proof]
    fn contract_binds_to_the_symbolic_host() {
        let vm = SymbolicVM::concrete_ctx();
        let v = Vesting::from(&vm);
        assert_eq!(v.total(), U256::ZERO);
    }


    #[kani::proof]
    #[kani::stub(ruint::Uint::overflowing_mul, kani_stylus_core::arith_oracle::overflowing_mul_stub)]
    fn initialize_runs_successfully() {
        let vm = SymbolicVM::concrete_ctx();
        let mut v = Vesting::from(&vm);

        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);

        // `initialize` now rejects schedules whose arithmetic would overflow, so
        // an `unwrap()` here would turn a legitimate rejection into a panic.
        // Assuming success is the harness's actual claim: *when* initialization
        // succeeds, the fields are set. The rejections have their own harness.
        let created = v.initialize(Address::from([1u8; 20]), any_u256(), start, duration);
        kani::assume(created.is_ok());

        assert_eq!(v.duration(), duration);
    }

    /// The second call must fail *because the contract is already initialized*.
    /// The new parameter checks can also reject it, but the assertion has to
    /// hold on every path, so paths with valid parameters still exercise the
    /// re-initialization guard specifically.
    #[kani::proof]
    #[kani::stub(ruint::Uint::overflowing_mul, kani_stylus_core::arith_oracle::overflowing_mul_stub)]
    fn cannot_reinitialize() {
        let vm = SymbolicVm::<4>::concrete_ctx();
        let mut v = Vesting::from(&vm);

        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);
        let total = any_u256();
        // See `initialize_runs_successfully` on why this is not an `unwrap()`.
        let created = v.initialize(Address::from([1u8; 20]), total, start, duration);
        kani::assume(created.is_ok());

        let start2: u64 = kani::any();
        let duration2: u64 = kani::any();
        kani::assume(duration2 > 0);
        let total2 = any_u256();
        assert!(v.initialize(Address::from([2u8; 20]), total2, start2, duration2).is_err());

        assert_eq!(v.beneficiary(), Address::from([1u8; 20]));
        assert_eq!(v.total(), total);
        assert_eq!(v.start(), start);
        assert_eq!(v.duration(), duration);
        assert_eq!(v.last_claim(), start);
    }
    
    #[kani::proof]
    #[kani::stub(ruint::Uint::overflowing_mul, kani_stylus_core::arith_oracle::overflowing_mul_stub)]
    fn initialize_rejects_zero_duration() {
        let vm = SymbolicVm::<4>::concrete_ctx();
        let mut v = Vesting::from(&vm);
        let start: u64 = kani::any();
        let total = any_u256();
        assert!(v.initialize(Address::from([1u8; 20]), total, start, 0).is_err());
        assert_eq!(v.beneficiary(), Address::ZERO);
        assert_eq!(v.total(), U256::ZERO);
    }

    /// DELIBERATE DEFECT 1, demonstrated.
    ///
    /// `vested_amount_unchecked` computes `start + duration` without checking,
    /// so a far-future schedule wraps: the end lands *below* the start, `t >=
    /// end` holds from the outset, and the contract reports everything vested
    /// immediately.
    ///
    /// This is the one defect Kani finds unaided — `u64` is primitive and
    /// arithmetic overflow checks are on by default — so the harness only has to
    /// stop assuming the case away. That is the contrast with
    /// `vested_unchecked_overflows`: the same bug in `U256`, invisible without
    /// an explicit assertion because `ruint`'s `*` is `wrapping_mul`.
    ///
    /// Seeds storage rather than calling `initialize`, necessarily —
    /// `initialize` rejects precisely this schedule, so it is the only way in.
    #[kani::proof]
    // Never reached — the overflow panics before the division — but the stub
    // costs nothing and keeps ruint's unbounded `wrapping_div` loops out of the
    // goto program regardless of path feasibility.
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::wrapping_div_stub)]
    #[kani::should_panic]
    fn far_future_schedule_overflows() {
        let vm = SymbolicVm::<4>::concrete_ctx();
        let mut v = Vesting::from(&vm);

        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);
        // The case every other harness excludes and `initialize` now refuses.
        kani::assume(start.checked_add(duration).is_none());

        v.start.set(U64::from(start));
        v.duration.set(U64::from(duration));
        v.total.set(any_u256());

        // Past the early return, so the overflowing addition is reached.
        let t: u64 = kani::any();
        kani::assume(start <= t);

        core::hint::black_box(v.vested_amount_unchecked(t));
    }

    /// The two schedule checks reject what they are there to reject, and the
    /// contract stays untouched.
    ///
    /// Without this they would be exercised only incidentally — every other
    /// harness either supplies parameters that pass them or assumes success —
    /// so a guard that stopped firing would leave the suite green.
    #[kani::proof]
    #[kani::stub(ruint::Uint::overflowing_mul, kani_stylus_core::arith_oracle::overflowing_mul_stub)]
    fn initialize_rejects_an_unworkable_schedule() {
        let vm = SymbolicVm::<4>::concrete_ctx();
        let mut v = Vesting::from(&vm);

        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);          // past the zero-duration guard
        let total = any_u256();

        // At least one of the two schedule checks has to reject this. The
        // `checked_mul` here goes through the same stub as the contract's, so
        // both see one recorded multiplication and agree on its overflow flag.
        kani::assume(
            start.checked_add(duration).is_none()
                || total.checked_mul(U256::from(duration)).is_none(),
        );

        assert!(
            v.initialize(Address::from([1u8; 20]), total, start, duration).is_err(),
            "initialize accepted a schedule whose arithmetic overflows"
        );
        assert_eq!(v.beneficiary(), Address::ZERO);
        assert_eq!(v.total(), U256::ZERO);
    }

    /// DELIBERATE DEFECT 4: initializing *to* the zero address leaves the
    /// re-initialization guard unlatched, so the contract can be initialized
    /// again.
    ///
    /// Asserted directly rather than through `#[kani::should_panic]`, because
    /// the defect is *universal* here: after `initialize(Address::ZERO, ..)` the
    /// guard `beneficiary != ZERO` is false on every path, so with workable
    /// parameters the second call always succeeds. `should_panic` is only
    /// needed where the defect is existential and the property can only be
    /// stated by negating it — `vested_unchecked_overflows` is that case.
    ///
    /// Stating it positively costs two assumptions and buys three things: no
    /// unrelated panic can satisfy the harness, every other assertion in it has
    /// to hold too, and it can use the oracle's `overflowing_mul_stub` instead
    /// of the exact one, keeping a 512-bit multiply out of the formula. It
    /// still catches a "fix": the second call would start returning `Err` and
    /// this assertion would fail.
    #[kani::proof]
    #[kani::stub(ruint::Uint::overflowing_mul, kani_stylus_core::arith_oracle::overflowing_mul_stub)]
    fn zero_beneficiary_never_latches() {
        let vm = SymbolicVm::<4>::concrete_ctx();
        let mut v = Vesting::from(&vm);
        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);
        let total = any_u256();

        // Not an `unwrap()`: `initialize` can now reject these parameters, and
        // that panic would satisfy `should_panic` without demonstrating
        // anything. The defect has to be what panics.
        let created = v.initialize(Address::ZERO, total, start, duration);
        kani::assume(created.is_ok());

        let start2: u64 = kani::any();
        let duration2: u64 = kani::any();
        kani::assume(duration2 > 0);
        let total2 = any_u256();
        // The second call's parameters have to be workable, or it would be
        // rejected by the schedule checks and the assertion below would fail
        // for a reason that has nothing to do with the guard.
        kani::assume(start2.checked_add(duration2).is_some());
        kani::assume(total2.checked_mul(U256::from(duration2)).is_some());

        assert!(
            v.initialize(Address::ZERO, total2, start2, duration2).is_ok(),
            "a zero beneficiary never latches the guard -- the contract stays re-initializable"
        );
    }


    /// Strictly `t < start`, which is why this needs no overflow guard on
    /// `start + duration`: `schedule` returns from its first branch before ever
    /// evaluating that sum, so DELIBERATE DEFECT 1 is unreachable here and the
    /// property holds for *every* `start` and `duration`, including ones that
    /// would overflow. `t == start` is covered separately by
    /// `vested_is_zero_at_the_start`, which does reach the sum and therefore
    /// does need the guard.
    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::wrapping_div_stub)]
    // `initialize` calls `checked_mul`; see `vested_is_zero_at_the_start`.
    #[kani::stub(ruint::Uint::overflowing_mul, kani_stylus_core::arith_oracle::overflowing_mul_stub)]
    fn nothing_vests_before_the_start() {
        let vm = SymbolicVm::<8>::concrete_ctx();
        let mut v = Vesting::from(&vm);

        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);

        let total = any_u256();

        // See `vested_never_exceeds_total`: `initialize` carries both guards now.
        let created = v.initialize(Address::from([1u8; 20]), total, start, duration);
        kani::assume(created.is_ok());

        let t: u64 = kani::any();
        kani::assume(t < start);

        assert_eq!(v.vested_amount(t), U256::ZERO);
    }


    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::wrapping_div_stub)]
    // `initialize` calls `checked_mul`, which routes to `overflowing_mul`.
    // Unstubbed that reaches ruint's `addmul` -> `trim_end_zeros`, whose loops
    // CBMC cannot bound, so the harness would hang rather than fail.
    #[kani::stub(ruint::Uint::overflowing_mul, kani_stylus_core::arith_oracle::overflowing_mul_stub)]
    fn vested_is_zero_at_the_start() {
        let vm = SymbolicVm::<8>::concrete_ctx();
        let mut v = Vesting::from(&vm);

        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);
        kani::assume(start.checked_add(duration).is_some());

        let total = any_u256();

        // See `vested_never_exceeds_total`: `initialize` carries both guards now.
        let created = v.initialize(Address::from([1u8; 20]), total, start, duration);
        kani::assume(created.is_ok());

        assert_eq!(v.vested_amount(start), U256::ZERO);
    }


    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::wrapping_div_stub)]
    // `initialize` calls `checked_mul`; see `vested_is_zero_at_the_start`.
    #[kani::stub(ruint::Uint::overflowing_mul, kani_stylus_core::arith_oracle::overflowing_mul_stub)]
    fn vested_is_total_after_end() {
        let vm = SymbolicVm::<8>::concrete_ctx();
        let mut v = Vesting::from(&vm);
        
        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);

        let total = any_u256();

        // See `vested_never_exceeds_total`: `initialize` carries both guards now.
        // `start + duration` below is safe for the same reason -- a contract
        // that exists has a schedule whose end does not overflow.
        let created = v.initialize(Address::from([1u8; 20]), total, start, duration);
        kani::assume(created.is_ok());

        let t: u64 = kani::any();
        kani::assume(t >= start + duration);

        assert_eq!(v.vested_amount(t), total);
    }

    /// The vested amount never goes backwards: for *every* pair of instants
    /// `t1 <= t2`, before the start, during the schedule and after its end, with
    /// every parameter symbolic and `total` bounded only by what `initialize`
    /// accepts, through the contract's own storage.
    ///
    /// Bit-precise this does not converge -- it rests on `*` and `/` being
    /// monotone, which a SAT solver would have to rediscover at 256 bits. With
    /// `kani_stylus_core::arith_oracle` it follows from a few lemmas: both are
    /// monotone, and `total * e / d <= total` while `e <= d`.
    ///
    /// The `cover` is the non-vacuity check, and it is deliberately confined
    /// to the division branch: before the start and after the end no `*` or
    /// `/` runs, so an increase there would say nothing about the oracle.
    /// Satisfying it means that, under the oracle's lemmas, vesting can still
    /// grow strictly mid-schedule -- the lemmas are consistent, and they have
    /// not made the assertion trivially true.
    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_mul, kani_stylus_core::arith_oracle::mul_stub)]
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::arith_oracle::div_stub)]
    #[kani::stub(ruint::Uint::overflowing_mul, kani_stylus_core::arith_oracle::overflowing_mul_stub)]
    fn vested_is_monotone_in_time() {
        let vm = SymbolicVm::<4>::concrete_ctx();
        let mut v = Vesting::from(&vm);

        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);
        let total = any_u256();

        // See `vested_never_exceeds_total`: `initialize` carries both guards now.
        let created = v.initialize(Address::from([1u8; 20]), total, start, duration);
        kani::assume(created.is_ok());

        let t1: u64 = kani::any();
        let t2: u64 = kani::any();
        kani::assume(t1 <= t2);

        let (early, late) = (v.vested_amount(t1), v.vested_amount(t2));
        assert!(early <= late, "monotonicity broken");
        kani::cover!(
            start < t1 && t2 < start + duration && early < late,
            "vesting grows mid-schedule"
        );
    }

    /// It rests on `total * e / d <= total` for `e <= d`, a nonlinear fact:
    /// with exact division this sat behind `slow-proofs`. It is one of
    /// `arith_oracle`'s lemmas.
    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_mul, kani_stylus_core::arith_oracle::mul_stub)]
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::arith_oracle::div_stub)]
    #[kani::stub(ruint::Uint::overflowing_mul, kani_stylus_core::arith_oracle::overflowing_mul_stub)]
    fn vested_never_exceeds_total() {
        let vm = SymbolicVm::<4>::concrete_ctx();
        let mut v = Vesting::from(&vm);

        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);
        let total = any_u256();

        // `initialize` enforces both schedule guards, so the harness no longer
        // restates them. A contract that exists has a workable schedule, and
        // that is a stronger starting point than hand-seeded storage: it is a
        // state the contract can actually be in.
        let created = v.initialize(Address::from([1u8; 20]), total, start, duration);
        kani::assume(created.is_ok());

        let t: u64 = kani::any();
        assert!(v.vested_amount(t) <= total, "vested exceeded total");
        // Non-vacuity: the branch that multiplies and divides is reachable
        // under the oracle's lemmas, not only the two that return early.
        kani::cover!(start < t && t < start + duration, "the division branch is reachable");
    }

    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::wrapping_div_stub)]
    #[kani::should_panic]
    fn vested_unchecked_overflows() {
        let vm = SymbolicVm::<4>::concrete_ctx();
        let mut v = Vesting::from(&vm);
        
        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);
        kani::assume(start.checked_add(duration).is_some());

        let total = any_u256();

        v.start.set(U64::from(start));
        v.duration.set(U64::from(duration));
        v.total.set(total);

        let t1: u64 = kani::any();
        let t2: u64 = kani::any();
        kani::assume(start <= t1);
        kani::assume(t1 <= t2);
        kani::assume(t2 <= start + duration);
        // The *unchecked* variant: defect 2 is the silent wrap, so demonstrating
        // it needs the function that wraps. The checked one would panic on the
        // guard instead, which `should_panic` would accept while showing nothing.
        assert!(
            v.vested_amount_unchecked(t1) <= v.vested_amount_unchecked(t2),
            "monotonicity broken"
        );
    }

    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_mul, kani_stylus_core::arith_oracle::mul_stub)]
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::arith_oracle::div_stub)]
    #[kani::stub(ruint::Uint::overflowing_mul, kani_stylus_core::arith_oracle::overflowing_mul_stub)]
    fn release_is_idempotent_at_the_same_timestamp() {
        let mut ctx = Context::concrete();
        ctx.block_timestamp = kani::any();
        let vm = SymbolicVm::<3>::with_context(ctx);
        let mut v = Vesting::from(&vm);

        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);
        let total = any_u256();

        // See `vested_never_exceeds_total`: `initialize` carries both guards now.
        let created = v.initialize(Address::from([1u8; 20]), total, start, duration);
        kani::assume(created.is_ok());
        // `released` left at its default zero -- unwritten slots read as zero.

        let first = v.release();
        let after_first = v.released();
        let second = v.release();

        assert_eq!(first, after_first, "released did not match the first payout");
        assert_eq!(second, U256::ZERO, "the second call paid out again");
        assert_eq!(v.released(), after_first, "released moved on the second call");
    }

    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::wrapping_div_stub)]
    // Division stays exact: the loss this demonstrates *is* floor-division
    // behaviour, so a fabricated quotient would prove nothing. Only the overflow
    // flag goes through the oracle, and `assume(created.is_ok())` pins it, so
    // nothing fabricated can reach the assertion.
    #[kani::stub(ruint::Uint::overflowing_mul, kani_stylus_core::arith_oracle::overflowing_mul_stub)]
    #[kani::should_panic]
    fn drifting_release_loses_value() {
        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        let t1: u64 = kani::any();
        let t2: u64 = kani::any();
        kani::assume(duration > 0);
        kani::assume(start <= t1);
        kani::assume(t1 < t2);                        // strictly later, or nothing accrues

        let total = any_u256();

        let mut ctx = Context::concrete();
        ctx.block_timestamp = t1;
        let vm1 = SymbolicVm::<4>::with_context(ctx);
        let mut v1 = Vesting::from(&vm1);

        // `initialize` carries both schedule guards and sets `last_claim` to
        // `start`, which is the pre-state this harness needs.
        let created = v1.initialize(Address::from([1u8; 20]), total, start, duration);
        kani::assume(created.is_ok());
        kani::assume(t2 <= start + duration);         // safe: the schedule end exists

        v1.release_drifting();

        // Same storage, later clock.
        let vm2 = vm1.with_timestamp(t2);
        let mut v2 = Vesting::from(&vm2);
        v2.release_drifting();

        assert!(
            v2.released_naive() >= v2.vested_amount(t2),
            "drifting kept up with the schedule"
        );
    }

    /// The core correctness property of the release side: after a claim,
    /// `released` is exactly what the schedule says has vested.
    ///
    /// Two divisions at the same instant — one inside `release`, one in the
    /// assertion — so this needs only that identical operands give identical
    /// quotients, never a fact relating different products.
    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_mul, kani_stylus_core::arith_oracle::mul_stub)]
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::arith_oracle::div_stub)]
    #[kani::stub(ruint::Uint::overflowing_mul, kani_stylus_core::arith_oracle::overflowing_mul_stub)]
    fn release_leaves_released_equal_to_vested() {
        let now: u64 = kani::any();
        let mut ctx = Context::concrete();
        ctx.block_timestamp = now;
        let vm = SymbolicVm::<3>::with_context(ctx);
        let mut v = Vesting::from(&vm);

        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);
        let total = any_u256();

        // See `vested_never_exceeds_total`: `initialize` carries both guards now.
        let created = v.initialize(Address::from([1u8; 20]), total, start, duration);
        kani::assume(created.is_ok());
        // `released` left at its default zero -- unwritten slots read as zero.

        v.release();

        assert_eq!(v.released(), v.vested_amount(now), "released did not land on the schedule");
    }

    /// Defect 3's partner, and what makes the loss *frequency-dependent* rather
    /// than simply wrong: claimed once from a fresh schedule, the two release
    /// paths agree exactly. `drifting_release_loses_value` shows them diverging
    /// once a second claim happens; neither harness says it alone.
    ///
    /// `initialize` sets `last_claim` to `start`, so both paths divide
    /// `total * (now - start)` by `duration` — identical operands, which is all
    /// the oracle needs to make the two quotients equal.
    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_mul, kani_stylus_core::arith_oracle::mul_stub)]
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::arith_oracle::div_stub)]
    #[kani::stub(ruint::Uint::overflowing_mul, kani_stylus_core::arith_oracle::overflowing_mul_stub)]
    fn release_and_drifting_agree_on_a_single_claim() {
        let now: u64 = kani::any();
        let mut ctx = Context::concrete();
        ctx.block_timestamp = now;
        let vm = SymbolicVm::<4>::with_context(ctx);
        let mut v = Vesting::from(&vm);

        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);
        let total = any_u256();

        // See `vested_never_exceeds_total`: `initialize` carries both guards.
        let created = v.initialize(Address::from([1u8; 20]), total, start, duration);
        kani::assume(created.is_ok());

        // `release_drifting` computes `now - last_claim`, so the clock must not
        // run backwards past the schedule's start.
        kani::assume(start <= now);
        kani::assume(now <= start + duration);

        let correct = v.release();
        let drifting = v.release_drifting();

        assert_eq!(correct, drifting, "the two paths disagreed on a single claim");
    }

    /// DELIBERATE DEFECT 5: `releasable` subtracts with a bare `-`, so it wraps
    /// once `released` exceeds what has vested. Unreachable through the
    /// contract's own methods — `release` only ever adds what `releasable` just
    /// returned — but reachable from seeded pre-state, which is exactly how a
    /// proof sets things up, and OpenZeppelin leans on the same invariant.
    ///
    /// Bounding the excess by `total` is what makes the defect universal, so it
    /// can be asserted outright rather than through `should_panic`. With
    /// `released` drawn freely the wrap lands anywhere in `[0, 2^256)` — take
    /// `vested_now == 0` and `released == U256::MAX` and `releasable()` comes
    /// back as `1`, which is within the schedule and proves nothing. A realistic
    /// over-release of at most one entitlement always wraps to at least
    /// `2^256 - total`, which is the case worth showing.
    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_mul, kani_stylus_core::arith_oracle::mul_stub)]
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::arith_oracle::div_stub)]
    #[kani::stub(ruint::Uint::overflowing_mul, kani_stylus_core::arith_oracle::overflowing_mul_stub)]
    fn releasable_wraps_catastrophically() {
        let now: u64 = kani::any();
        let mut ctx = Context::concrete();
        ctx.block_timestamp = now;
        let vm = SymbolicVm::<3>::with_context(ctx);
        let mut v = Vesting::from(&vm);

        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);
        kani::assume(start.checked_add(duration).is_some());   // defect 1 guard
        let total = any_u256();
        kani::assume(total.checked_mul(U256::from(duration)).is_some());   // defect 2 guard
        // The claim below needs `2^256 - total > total`, so it holds only while
        // `total` is under half the `U256` range. Not a tractability hedge: above
        // that, an entitlement's worth of over-release wraps to something
        // *smaller* than the entitlement and the defect stops being visible this
        // way. A token whose supply exceeds 2^255 has stranger problems.
        kani::assume(total <= U256::MAX >> 1);

        v.start.set(U64::from(start));
        v.duration.set(U64::from(duration));
        v.total.set(total);

        // Seed a state the contract cannot reach on its own: more released than
        // has vested, by at most the whole entitlement.
        let vested_now = v.vested_amount(now);
        let released = any_u256();
        kani::assume(released > vested_now);
        kani::assume(released - vested_now <= total);
        v.released.set(released);
        kani::cover!(released > vested_now, "the over-released state is reachable");

        // `vested_now - released` wraps to `2^256 - (released - vested_now)`,
        // and the excess is at most `total`, so the result is at least
        // `2^256 - total`. An entitlement's worth of over-release turns a debt
        // into very nearly the entire `U256` range.
        assert!(
            v.releasable() > total,
            "the bare `-` did not wrap -- defect 5 is gone"
        );
    }

    /// The contract's top-level safety invariant: however often the beneficiary
    /// claims, and whenever, `released` never exceeds `total`.
    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::arith_oracle::div_stub)]
    #[kani::stub(ruint::Uint::wrapping_mul, kani_stylus_core::arith_oracle::mul_stub)]
    #[kani::stub(ruint::Uint::overflowing_mul, kani_stylus_core::arith_oracle::overflowing_mul_stub)]
    fn released_never_exceeds_total() {
        let now: u64 = kani::any();
        let mut ctx = Context::concrete();
        ctx.block_timestamp = now;
        let vm = SymbolicVm::<4>::with_context(ctx);
        let mut v = Vesting::from(&vm);

        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);
        let total = any_u256();

        // See `vested_never_exceeds_total`: `initialize` carries both guards now.
        let created = v.initialize(Address::from([1u8; 20]), total, start, duration);
        kani::assume(created.is_ok());

        // Two claims, the clock only moving forward. One claim would reduce to
        // `vested <= total` and miss the accumulation the invariant is about.
        let later: u64 = kani::any();
        kani::assume(now <= later);

        core::hint::black_box(v.release());

        let vm2 = vm.with_timestamp(later);
        let mut v2 = Vesting::from(&vm2);
        core::hint::black_box(v2.release());

        assert!(v2.released() <= total, "released exceeded the total");
    }
}