extern crate alloc;
use stylus_sdk::{alloy_primitives::U256, prelude::*};

sol_storage! {
    #[entrypoint]
    pub struct Counter { uint256 number; }
}

#[public]
impl Counter {
    pub fn number(&self) -> U256 { self.number.get() }
    pub fn set_number(&mut self, n: U256) { self.number.set(n); }
    pub fn add_number(&mut self, n: U256) { self.number.set(n + self.number.get()); }
}

#[cfg(kani)]
mod symbolic_vm;

// Staged harnesses: each adds one layer, to locate where verification cost explodes.
#[cfg(kani)]
mod proofs {
    use super::*;
    use stylus_sdk::testing::*;

    /// Baseline: measures fixed overhead only.
    #[kani::proof]
    fn h0_empty() {
        let n: u64 = kani::any();
        assert_eq!(n, n);
    }

    /// Adds TestVM construction (9 HashMaps in VMState::default()).
    #[kani::proof]
    fn h1_vm_only() {
        let vm = TestVM::default();
        core::hint::black_box(&vm);
    }

    /// Adds contract binding to the host.
    #[kani::proof]
    fn h2_contract_only() {
        let vm = TestVM::default();
        let c = Counter::from(&vm);
        core::hint::black_box(&c);
    }

    /// Adds one symbolic storage read (HashMap::get on a U256 key).
    #[kani::proof]
    fn h3_read_only() {
        let vm = TestVM::default();
        let c = Counter::from(&vm);
        core::hint::black_box(c.number());
    }

    /// The original: one symbolic write then a read.
    #[kani::proof]
    fn h4_set_then_get() {
        let vm = TestVM::default();
        let mut c = Counter::from(&vm);
        let n: u64 = kani::any();
        c.set_number(U256::from(n));
        assert_eq!(c.number(), U256::from(n));
    }
}

/// The same properties, but against `SymbolicVM` instead of `TestVM` — i.e. an
/// array-backed slot store instead of nine `std::HashMap`s.
///
/// Compare `s3_set_then_get` against `h4_set_then_get` to see what the host
/// implementation costs.
#[cfg(kani)]
mod symbolic_proofs {
    use super::symbolic_vm::SymbolicVM;
    use super::*;

    #[kani::proof]
    fn s1_contract_only() {
        let vm = SymbolicVM::with_concrete_ctx();
        let c = Counter::from(&vm);
        core::hint::black_box(&c);
    }

    #[kani::proof]
    fn s2_read_only() {
        let vm = SymbolicVM::with_concrete_ctx();
        let c = Counter::from(&vm);
        core::hint::black_box(c.number());
    }

    /// A fresh contract's counter reads as zero.
    #[kani::proof]
    fn s3_starts_at_zero() {
        let vm = SymbolicVM::with_concrete_ctx();
        let c = Counter::from(&vm);
        assert_eq!(c.number(), U256::ZERO);
    }

    /// Storage round-trips: whatever you write is what you read back.
    #[kani::proof]
    fn s4_set_then_get() {
        let vm = SymbolicVM::with_concrete_ctx();
        let mut c = Counter::from(&vm);
        let n: u64 = kani::any();
        c.set_number(U256::from(n));
        assert_eq!(c.number(), U256::from(n));
    }

    /// The real target: `add_number` must not silently wrap. Kani's default
    /// overflow checks make this a genuine property, not a tautology —
    /// `U256::add` panics on overflow, so this proves it is unreachable
    /// under the assumption.
    #[kani::proof]
    fn s5_add_no_overflow() {
        let vm = SymbolicVM::with_concrete_ctx();
        let mut c = Counter::from(&vm);
        let a: u64 = kani::any();
        let b: u64 = kani::any();
        c.set_number(U256::from(a));
        c.add_number(U256::from(b));
        assert_eq!(c.number(), U256::from(a) + U256::from(b));
    }

    /// Full symbolic transaction context, to price what that costs.
    #[kani::proof]
    fn s6_symbolic_ctx() {
        let vm = SymbolicVM::new();
        let mut c = Counter::from(&vm);
        let n: u64 = kani::any();
        c.set_number(U256::from(n));
        assert_eq!(c.number(), U256::from(n));
    }
}

/// Counterexample generation, and the reason this project is needed.
///
/// `alloy`/`ruint` define `U256 + U256` as `wrapping_add`
/// (`impl_bin_op!(Add, add, AddAssign, add_assign, wrapping_add)` in
/// `ruint/src/add.rs`). It never panics. And Kani's automatic
/// arithmetic-overflow checks only fire on *primitive* integer operations —
/// `U256` is a struct whose limbs are combined with explicitly-wrapping
/// `carrying_add`, so nothing fires there either.
///
/// Net effect: **a Stylus contract silently wraps on U256 overflow, and Kani
/// will not catch it for free.** Solidity >=0.8 reverts on this. Overflow has
/// to be stated as an explicit property, which is exactly what a property
/// library for Stylus should provide.
#[cfg(kani)]
mod defects {
    use super::symbolic_vm::SymbolicVM;
    use super::*;

    /// Expected to FAIL, with a concrete counterexample.
    ///
    /// Adding a non-negative amount must never *decrease* the counter. It does,
    /// because the addition wraps. This is a real bug in the stock Stylus
    /// counter template, found automatically.
    ///
    /// `cargo kani --harness d1_add_number_can_decrease_the_counter \
    ///     --concrete-playback=print` turns the witness into a runnable test.
    #[kani::proof]
    #[kani::should_panic]
    fn d1_add_number_can_decrease_the_counter() {
        let vm = SymbolicVM::with_concrete_ctx();
        let mut c = Counter::from(&vm);
        let a = U256::from_be_bytes(kani::any::<[u8; 32]>());
        let b = U256::from_be_bytes(kani::any::<[u8; 32]>());
        c.set_number(a);
        c.add_number(b);
        assert!(c.number() >= a, "add_number decreased the counter");
    }

    /// Pins down the actual semantics: the result is exactly the wrapped sum.
    /// Passing this is what proves `d1` is a wrap and not something else.
    #[kani::proof]
    fn d2_add_number_is_exactly_wrapping() {
        let vm = SymbolicVM::with_concrete_ctx();
        let mut c = Counter::from(&vm);
        let a = U256::from_be_bytes(kani::any::<[u8; 32]>());
        let b = U256::from_be_bytes(kani::any::<[u8; 32]>());
        c.set_number(a);
        c.add_number(b);
        assert_eq!(c.number(), a.wrapping_add(b));
    }

    /// Control: with overflow ruled out by assumption, the method is exact.
    /// This is the shape a real proof obligation should take.
    #[kani::proof]
    fn d3_add_number_exact_when_no_overflow() {
        let vm = SymbolicVM::with_concrete_ctx();
        let mut c = Counter::from(&vm);
        let a = U256::from_be_bytes(kani::any::<[u8; 32]>());
        let b = U256::from_be_bytes(kani::any::<[u8; 32]>());
        kani::assume(a.checked_add(b).is_some());
        c.set_number(a);
        c.add_number(b);
        assert_eq!(c.number(), a + b);
        assert!(c.number() >= a);
    }
}
