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
//! `U256` division is otherwise enough to make a proof diverge. See
//! [`kani_stylus_core::arith`].
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

#[inline(always)]
fn schedule(start: u64, duration: u64, total: U256, t: u64) -> U256 {
    if t < start {
        return U256::ZERO;
    }

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
        schedule(self.start(), self.duration(), self.total.get(), t)
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
    // The `wrapping_div_stub*` family is named by full path in the
    // `#[kani::stub(..)]` attributes, so it needs no import here.
    use kani_stylus_core::{any_u256, split, ueq, ult, widen, SymbolicVM, SymbolicVm};

    // Only the `slow-proofs` harnesses build a custom context.
    #[cfg(feature = "slow-proofs")]
    use kani_stylus_core::Context;

    use ruint::Uint;

    // NOTE: !!! do *NOT* just remove, presumably needed for proper total overflow test (currently replaced with simplified bit-shift test or not applied at all in each harness)
    fn overflowing_mul_stub<const BITS: usize, const LIMBS: usize>(
        a: Uint<BITS, LIMBS>, b: Uint<BITS, LIMBS>,
    ) -> (Uint<BITS, LIMBS>, bool) {
        const { assert!(LIMBS >= 1 && BITS <= 256) };
        split(widen(a) * widen(b))
    }

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
    fn initialize_runs_successfully() {
        let vm = SymbolicVM::concrete_ctx();
        let mut v = Vesting::from(&vm);

        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);
        v.initialize(Address::from([1u8; 20]), any_u256(), start, duration).unwrap();
        assert_eq!(v.duration(), duration);
    }

    #[kani::proof]
    fn cannot_reinitialize() {
        let vm = SymbolicVm::<4>::concrete_ctx();
        let mut v = Vesting::from(&vm);

        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);
        let total = any_u256();
        v.initialize(Address::from([1u8; 20]), total, start, duration).unwrap();

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
    fn initialize_rejects_zero_duration() {
        let vm = SymbolicVm::<4>::concrete_ctx();
        let mut v = Vesting::from(&vm);
        let start: u64 = kani::any();
        let total = any_u256();
        assert!(v.initialize(Address::from([1u8; 20]), total, start, 0).is_err());
        assert_eq!(v.beneficiary(), Address::ZERO);
        assert_eq!(v.total(), U256::ZERO);
    }

    #[kani::proof]
    #[kani::should_panic]
    fn zero_beneficiary_never_latches() {
        let vm = SymbolicVm::<4>::concrete_ctx();
        let mut v = Vesting::from(&vm);
        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);
        let total = any_u256();

        v.initialize(Address::ZERO, total, start, duration).unwrap();

        let start2: u64 = kani::any();
        let duration2: u64 = kani::any();
        kani::assume(duration2 > 0);
        let total2 = any_u256();
        assert!(
            v.initialize(Address::ZERO, total2, start2, duration2).is_err(),
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
    fn nothing_vests_before_the_start() {
        let vm = SymbolicVm::<8>::concrete_ctx();
        let mut v = Vesting::from(&vm);

        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);

        let total = any_u256();
        v.start.set(U64::from(start));
        v.duration.set(U64::from(duration));
        v.total.set(total);

        let t: u64 = kani::any();
        kani::assume(t < start);

        assert_eq!(v.vested_amount(t), U256::ZERO);
    }


    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::wrapping_div_stub)]
    fn vested_is_zero_at_the_start() {
        let vm = SymbolicVm::<8>::concrete_ctx();
        let mut v = Vesting::from(&vm);

        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);
        kani::assume(start.checked_add(duration).is_some());

        let total = any_u256();
        v.start.set(U64::from(start));
        v.duration.set(U64::from(duration));
        v.total.set(total);

        assert_eq!(v.vested_amount(start), U256::ZERO);
    }


    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::wrapping_div_stub)]
    fn vested_is_total_after_end() {
        let vm = SymbolicVm::<8>::concrete_ctx();
        let mut v = Vesting::from(&vm);
        
        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);
        kani::assume(start.checked_add(duration).is_some());

        let total = any_u256();
        v.start.set(U64::from(start));
        v.duration.set(U64::from(duration));
        v.total.set(total);

        let t: u64 = kani::any();
        kani::assume(t >= start + duration);

        assert_eq!(v.vested_amount(t), total);
    }

    /*
    // NOTE: does not currently verify anything
    #[kani::proof]
    #[kani::stub(ruint::Uint::overflowing_mul, crate::proofs::overflowing_mul_stub)]
    fn mul_stub_takes_effect() {
        assert!(U256::MAX.checked_mul(U256::from(2u64)).is_none())
    }

    // NOTE: never converged, probably not the way to go
    #[kani::proof]
    #[kani::stub(ruint::Uint::overflowing_mul, crate::proofs::overflowing_mul_stub)]
    fn overflowing_mul_value_matches_wrapping_mul() {
        let a = any_u256();
        let b = any_u256();
        let (v, _) = a.overflowing_mul(b);
        assert_eq!(v, a.wrapping_mul(b));
    }
    */



    #[cfg(feature = "slow-proofs")]
    #[kani::proof]
    fn division_is_monotone_u64() {
        let a1: u64 = kani::any();
        let a2: u64 = kani::any();
        let b:  u64 = kani::any();
        kani::assume(b != 0);
        kani::assume(a1 <= a2);
        assert!(a1 / b <= a2 / b, "division not monotone");
    }


    #[cfg(feature = "slow-proofs")]
    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::wrapping_div_stub)]
    fn division_is_monotone_u256() {
        let a1 = any_u256();
        let a2 = any_u256();
        let b  = any_u256();
        kani::assume(b != U256::ZERO);
        kani::assume(a1 <= a2);
        assert!(a1 / b <= a2 / b, "division not monotone");
    }

    // DOES NOT CONVERGE. Gated so it stays out of the default suite.
    #[cfg(feature = "slow-proofs")]
    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::wrapping_div_stub_monotone)]
    #[kani::stub(ruint::Uint::overflowing_mul, crate::proofs::overflowing_mul_stub)]
    fn vested_is_monotone_in_time() {
        // NOTE: currently does not converge sufficiently fast!
        //let vm = SymbolicVm::<4>::concrete_ctx();
        //let mut v = Vesting::from(&vm);
        
        let start: u64 = 100u64; //kani::any();
        let duration: u64 = 1000u64; //kani::any();
        kani::assume(duration > 0);
        kani::assume(start.checked_add(duration).is_some());

        let total = U256::from(10000u64); //any_u256();
        //let total: u64 = kani::any();
        //kani::assume(total.checked_mul(U256::from(duration)).is_some()); // no overflow assumption
        kani::assume(total <= U256::MAX >> 64); // simplified no overflow assumption

        //v.start.set(U64::from(start));
        //v.duration.set(U64::from(duration));
        //v.total.set(U256::from(total));

        let t1: u64 = kani::any();
        let t2: u64 = kani::any();
        kani::assume(start <= t1);
        kani::assume(t1 <= t2);
        kani::assume(t2 <= start + duration);
        //assert!(v.vested_amount(t1) <= v.vested_amount(t2), "monotonicity broken");
        assert!(schedule(start, duration, total, t1) <= schedule(start, duration, total, t2), "monotonicity broken");
    }

    #[cfg(feature = "slow-proofs")]
    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::wrapping_div_stub_monotone)]
    #[kani::stub(ruint::Uint::overflowing_mul, crate::proofs::overflowing_mul_stub)]
    fn vested_experiment_0100() {
        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);
        kani::assume(start.checked_add(duration).is_some());
        let total = U256::from(100u64);

        let t1: u64 = kani::any();
        let t2: u64 = kani::any();
        kani::assume(start <= t1);
        kani::assume(t1 <= t2);
        kani::assume(t2 <= start + duration);
        assert!(schedule(start, duration, total, t1) <= schedule(start, duration, total, t2), "monotonicity broken");
    }

    #[cfg(feature = "slow-proofs")]
    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::wrapping_div_stub_monotone)]
    #[kani::stub(ruint::Uint::overflowing_mul, crate::proofs::overflowing_mul_stub)]
    fn vested_experiment_1000() {
        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);
        kani::assume(start.checked_add(duration).is_some());
        let total = U256::from(1000u64);

        let t1: u64 = kani::any();
        let t2: u64 = kani::any();
        kani::assume(start <= t1);
        kani::assume(t1 <= t2);
        kani::assume(t2 <= start + duration);
        assert!(schedule(start, duration, total, t1) <= schedule(start, duration, total, t2), "monotonicity broken");
    }

    #[cfg(feature = "slow-proofs")]
    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::wrapping_div_stub)]
    #[kani::stub(ruint::Uint::overflowing_mul, crate::proofs::overflowing_mul_stub)]
    fn vested_never_exceeds_total() {
        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);
        kani::assume(start.checked_add(duration).is_some());
        let total = any_u256();

        // Cheap over-approximation of "DELIBERATE DEFECT 2 cannot fire here":
        // since `t - start <= duration < 2^64`, `total <= 2^192 - 1` suffices.
        // Stronger than necessary — it excludes schedules that would not in fact
        // overflow. The exact form is
        // `total.checked_mul(U256::from(duration)).is_some()`, which needs the
        // `overflowing_mul` stub because `checked_mul` routes through it.
        kani::assume(total <= U256::MAX >> 64);

        let t: u64 = kani::any();
        assert!(schedule(start, duration, total, t) <= total, "vested exceeded total");
    }

    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::wrapping_div_stub)]
    #[kani::stub(ruint::Uint::overflowing_mul, crate::proofs::overflowing_mul_stub)]
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
        assert!(v.vested_amount(t1) <= v.vested_amount(t2), "monotonicity broken");
    }

    #[cfg(feature = "slow-proofs")]
    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::wrapping_div_stub_memo)]
    fn release_is_idempotent_at_the_same_timestamp() {
        let mut ctx = Context::concrete();
        ctx.block_timestamp = kani::any();
        let vm = SymbolicVm::<3>::with_context(ctx);
        let mut v = Vesting::from(&vm);

        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        kani::assume(duration > 0);
        kani::assume(start.checked_add(duration).is_some());   // defect 1 guard
        let total = U256::from(100u64);
        //let total = any_u256();

        v.start.set(U64::from(start));
        v.duration.set(U64::from(duration));
        v.total.set(total);
        // `released` left at its default zero -- unwritten slots read as zero.

        let first = v.release();
        let after_first = v.released();
        let second = v.release();

        assert_eq!(first, after_first, "released did not match the first payout");
        assert_eq!(second, U256::ZERO, "the second call paid out again");
        assert_eq!(v.released(), after_first, "released moved on the second call");
        //kani::cover!(vm.slots_touched() == 3);   // total, released, packed uint64 word

    }

    #[cfg(feature = "slow-proofs")]
    #[kani::proof]
    #[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::wrapping_div_stub)]
    #[kani::should_panic]
    fn drifting_release_loses_value() {
        let start: u64 = kani::any();
        let duration: u64 = kani::any();
        let t1: u64 = kani::any();
        let t2: u64 = kani::any();
        kani::assume(duration > 0);
        kani::assume(start.checked_add(duration).is_some());
        kani::assume(start <= t1);
        kani::assume(t1 < t2);                        // strictly later, or nothing accrues
        kani::assume(t2 <= start + duration);

        let total = any_u256();
        // Over-approximation of "DELIBERATE DEFECT 2 cannot fire here"; see the
        // note in `vested_never_exceeds_total` for why this is stronger than
        // necessary and what the exact form costs.
        kani::assume(total <= U256::MAX >> 64);

        let mut ctx = Context::concrete();
        ctx.block_timestamp = t1;
        let vm1 = SymbolicVm::<4>::with_context(ctx);
        let mut v1 = Vesting::from(&vm1);

        v1.start.set(U64::from(start));
        v1.duration.set(U64::from(duration));
        v1.total.set(total);
        v1.last_claim.set(U64::from(start));          // what `initialize` guarantees
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
}