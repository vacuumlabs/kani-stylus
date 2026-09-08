//! Stylus Hello World
//!
//! The following contract implements the Counter example from Foundry.
//!
//! ```solidity
//! contract Counter {
//!     uint256 public number;
//!     function setNumber(uint256 newNumber) public {
//!         number = newNumber;
//!     }
//!     function increment() public {
//!         number++;
//!     }
//! }
//! ```
//!
//! The program is ABI-equivalent with Solidity, which means you can call it from both Solidity and Rust.
//! To do this, run `cargo stylus export-abi`.
//!
//! Note: this code is a template-only and has not been audited.

// Allow `cargo stylus export-abi` to generate a main function.
#![cfg_attr(not(any(test, feature = "export-abi")), no_main)]
extern crate alloc;

/// Import items from the SDK. The prelude contains common traits and macros.
use stylus_sdk::{alloy_primitives::U256, prelude::*};

// Define some persistent storage using the Solidity ABI.
// `Counter` will be the entrypoint.
sol_storage! {
    #[entrypoint]
    pub struct Counter {
        uint256 number;
    }
}

/// Declare that `Counter` is a contract with the following external methods.
#[public]
impl Counter {
    /// Gets the number from storage.
    pub fn number(&self) -> U256 {
        self.number.get()
    }

    /// Sets a number in storage to a user-specified value.
    pub fn set_number(&mut self, new_number: U256) {
        self.number.set(new_number);
    }

    /// Multiplies `number` by `new_number` and updates its value in storage.
    pub fn mul_number(&mut self, new_number: U256) {
        self.number.set(new_number * self.number.get());
    }

    /// Increments `number` by `new_number` and updates its value in storage.
    pub fn add_number(&mut self, new_number: U256) {
        self.number.set(new_number + self.number.get());
    }

    /// Increments `number` by 1 and updates its value in storage.
    pub fn increment(&mut self) {
        let number = self.number.get();
        self.set_number(number + U256::from(1));
    }

    /// Adds the wei value from msg_value to the number in storage.
    #[payable]
    pub fn add_from_msg_value(&mut self) {
        let number = self.number.get();
        self.set_number(number + self.vm().msg_value());
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_counter() {
        use stylus_sdk::testing::*;
        let vm = TestVM::default();
        let mut contract = Counter::from(&vm);

        assert_eq!(U256::ZERO, contract.number());

        contract.increment();
        assert_eq!(U256::from(1), contract.number());

        contract.add_number(U256::from(3));
        assert_eq!(U256::from(4), contract.number());

        contract.mul_number(U256::from(2));
        assert_eq!(U256::from(8), contract.number());

        contract.set_number(U256::from(100));
        assert_eq!(U256::from(100), contract.number());

        // Override the msg value for future contract method invocations.
        vm.set_value(U256::from(2));

        contract.add_from_msg_value();
        assert_eq!(U256::from(102), contract.number());
    }
}

#[cfg(all(kani, not(feature = "proofs")))]
compile_error!("proofs need the `proofs` feature: cargo kani --features proofs");

/// Formal proofs, checked by `cargo kani --features proofs`.
///
/// These live in the ordinary contract crate alongside the `#[cfg(test)]`
/// module above. They compile only under verification, so `cargo test`,
/// `cargo build` and `cargo stylus check` are unaffected — the deployed wasm
/// contains none of this.
///
/// Where a unit test pins one input, a proof covers the whole input space.
/// `test_counter` above checks `add_number(3)` on a counter holding 1; the
/// proofs below check every `U256`, and find a bug in this template while
/// doing so.
#[cfg(kani)]
mod proofs {
    use super::*;
    use kani_stylus_core::{any_u256, SymbolicVM};

    /// Whatever you store is what you read back — for every value.
    #[kani::proof]
    fn set_then_get_roundtrips() {
        let vm = SymbolicVM::new();
        let mut contract = Counter::from(&vm);

        let n = any_u256();
        contract.set_number(n);

        assert_eq!(contract.number(), n);
    }

    /// A fresh contract reads zero: unwritten slots behave like the EVM's.
    #[kani::proof]
    fn starts_at_zero() {
        let vm = SymbolicVM::new();
        let contract = Counter::from(&vm);
        assert_eq!(contract.number(), U256::ZERO);
    }

    // -- the bugs -----------------------------------------------------------
    //
    // `alloy`'s `U256 + U256` is `wrapping_add`; it never panics. Kani's
    // automatic overflow checks only cover *primitive* integers, so they say
    // nothing about `U256` either. Solidity >= 0.8 reverts on this; here it is
    // silent. These harnesses are marked `should_panic` so the suite stays
    // green — remove the attribute to watch them fail.

    /// Adding to the counter can make it *smaller*.
    ///
    /// ```text
    /// cargo kani --features proofs -Z concrete-playback \
    ///     --concrete-playback=print --harness proofs::add_number_can_decrease_the_counter
    /// ```
    #[kani::proof]
    #[kani::should_panic]
    fn add_number_can_decrease_the_counter() {
        let vm = SymbolicVM::new();
        let mut contract = Counter::from(&vm);

        let a = any_u256();
        let b = any_u256();
        contract.set_number(a);
        contract.add_number(b);

        assert!(contract.number() >= a, "add_number shrank the counter");
    }

    /// `mul_number` wraps too.
    #[kani::proof]
    #[kani::should_panic]
    fn mul_number_can_wrap() {
        let vm = SymbolicVM::new();
        let mut contract = Counter::from(&vm);

        let a = any_u256();
        let b = any_u256();
        kani::assume(a != U256::ZERO && b != U256::ZERO);
        contract.set_number(a);
        contract.mul_number(b);

        // With no wrap, multiplying by a non-zero value cannot go below `a`
        // unless `b` is 1 or less. Restrict to b >= 2 to make that precise.
        kani::assume(b >= U256::from(2));
        assert!(contract.number() > a, "mul_number wrapped");
    }

    /// `increment` wraps, but only from exactly `U256::MAX` — a single point in
    /// a 2^256 space. A fuzzer would have to guess it; the solver derives it.
    #[kani::proof]
    #[kani::should_panic]
    fn increment_wraps_at_max() {
        let vm = SymbolicVM::new();
        let mut contract = Counter::from(&vm);

        let n = any_u256();
        contract.set_number(n);
        contract.increment();

        assert!(contract.number() > n, "increment did not increase");
    }

    // -- what a real proof obligation looks like -----------------------------

    /// The shape to copy: state the precondition, then assert the property.
    /// Everything after `kani::assume` is proved for all inputs that satisfy it.
    #[kani::proof]
    fn add_number_is_exact_when_it_does_not_overflow() {
        let vm = SymbolicVM::new();
        let mut contract = Counter::from(&vm);

        let a = any_u256();
        let b = any_u256();
        kani::assume(a.checked_add(b).is_some());

        contract.set_number(a);
        contract.add_number(b);

        assert_eq!(contract.number(), a + b);
        assert!(contract.number() >= a);
    }

    /// `add_from_msg_value` is `#[payable]`, so its behaviour depends on the
    /// transaction. `SymbolicVM::new()` supplies a symbolic `msg_value`, so
    /// this covers every possible attached value at once.
    #[kani::proof]
    fn add_from_msg_value_adds_exactly_the_value_sent() {
        let start = any_u256();
        let sent = any_u256();
        kani::assume(start.checked_add(sent).is_some());

        let vm = SymbolicVM::new().with_value(sent);
        let mut contract = Counter::from(&vm);
        contract.set_number(start);

        contract.add_from_msg_value();

        assert_eq!(contract.number(), start + sent);
    }
}
