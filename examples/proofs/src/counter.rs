//! The stock `cargo stylus new` counter, verified.
//!
//! Nothing here is contrived. This is the template Arbitrum hands every new
//! Stylus developer, and it has a real bug in it.

use stylus_sdk::{alloy_primitives::U256, prelude::*};

sol_storage! {
    pub struct Counter {
        uint256 number;
    }
}

#[public]
impl Counter {
    pub fn number(&self) -> U256 {
        self.number.get()
    }

    pub fn set_number(&mut self, new_number: U256) {
        self.number.set(new_number);
    }

    pub fn add_number(&mut self, new_number: U256) {
        self.number.set(new_number + self.number.get());
    }

    pub fn increment(&mut self) {
        let number = self.number.get();
        self.set_number(number + U256::from(1));
    }
}

#[cfg(kani)]
mod proofs {
    use super::*;
    use kani_stylus_core::{any_u256, SymbolicVM};

    /// Start here. A property over *every* `U256`, not one example value.
    ///
    /// `cargo kani --harness counter::proofs::set_then_get_roundtrips`
    #[kani::proof]
    fn set_then_get_roundtrips() {
        let vm = SymbolicVM::concrete_ctx();
        let mut c = Counter::from(&vm);

        let n = any_u256();
        c.set_number(n);

        assert_eq!(c.number(), n);
    }

    /// A fresh contract reads zero — unwritten slots behave like the EVM's.
    #[kani::proof]
    fn starts_at_zero() {
        let vm = SymbolicVM::concrete_ctx();
        let c = Counter::from(&vm);
        assert_eq!(c.number(), U256::ZERO);
    }

    // ---- the bug -----------------------------------------------------------

    /// **This is the point of the whole project.**
    ///
    /// Adding to a counter should never make it smaller. It can, because
    /// `alloy`'s `U256 + U256` is `wrapping_add` — it never panics — and Kani's
    /// automatic overflow checks only cover *primitive* integers, not library
    /// types built on wrapping `u64` limbs. So neither `cargo test` nor a naive
    /// `cargo kani` run flags this. Solidity >= 0.8 would revert.
    ///
    /// Marked `should_panic` so the suite stays green; drop that attribute to
    /// see it fail, and get the witness with:
    ///
    /// ```text
    /// cargo kani -Z concrete-playback --concrete-playback=print \
    ///     --harness counter::proofs::add_number_can_decrease_the_counter
    /// ```
    #[kani::proof]
    #[kani::should_panic]
    fn add_number_can_decrease_the_counter() {
        let vm = SymbolicVM::concrete_ctx();
        let mut c = Counter::from(&vm);

        let a = any_u256();
        let b = any_u256();
        c.set_number(a);
        c.add_number(b);

        assert!(c.number() >= a, "add_number made the counter smaller");
    }

    /// Pins the semantics down: the result is *exactly* the wrapped sum.
    ///
    /// Without this, `add_number_can_decrease_the_counter` only tells you
    /// something is wrong, not what.
    #[kani::proof]
    fn add_number_is_exactly_wrapping() {
        let vm = SymbolicVM::concrete_ctx();
        let mut c = Counter::from(&vm);

        let a = any_u256();
        let b = any_u256();
        c.set_number(a);
        c.add_number(b);

        assert_eq!(c.number(), a.wrapping_add(b));
    }

    // ---- what a real proof obligation looks like ---------------------------

    /// The shape to copy. State the precondition, then assert the property.
    ///
    /// `kani::assume` narrows the input space to the cases the caller is
    /// expected to respect; everything after it is proved for all of them.
    #[kani::proof]
    fn add_number_is_exact_when_it_does_not_overflow() {
        let vm = SymbolicVM::concrete_ctx();
        let mut c = Counter::from(&vm);

        let a = any_u256();
        let b = any_u256();
        kani::assume(a.checked_add(b).is_some());

        c.set_number(a);
        c.add_number(b);

        assert_eq!(c.number(), a + b);
        assert!(c.number() >= a);
    }

    /// `increment` has the same latent wrap, reachable only from `U256::MAX`.
    /// A fuzzer would need to guess exactly that value; the solver derives it.
    #[kani::proof]
    #[kani::should_panic]
    fn increment_wraps_at_max() {
        let vm = SymbolicVM::concrete_ctx();
        let mut c = Counter::from(&vm);

        let n = any_u256();
        c.set_number(n);
        c.increment();

        assert!(c.number() > n, "increment did not increase the counter");
    }

    /// Same round-trip, but with a fully symbolic sender, value and block.
    /// Costs roughly twice the solver time; use it when a property might
    /// depend on transaction context.
    #[kani::proof]
    fn roundtrip_holds_for_any_transaction_context() {
        let vm = SymbolicVM::new();
        let mut c = Counter::from(&vm);

        let n = any_u256();
        c.set_number(n);

        assert_eq!(c.number(), n);
    }
}
