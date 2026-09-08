//! A small vault, verified with Kani.
//!
//! This is an ordinary Stylus contract — `cargo stylus new vault`, with the
//! contract body written and proofs added in place. It covers two things the
//! [counter example](../../counter) has no occasion for:
//!
//! - **access control** over a symbolic caller, so `only_owner_can_transfer`
//!   holds for *every* address rather than a handful of test accounts;
//! - **mappings**, whose slots are keccak-derived, which exercises the hash
//!   oracle in `kani_stylus_core::keccak`.
//!
//! ```bash
//! cargo test                                      # ordinary unit tests
//! cargo kani --features proofs -Z stubbing        # the proofs
//! ```
//!
//! `-Z stubbing` is required here and not in the counter: Stylus mappings hash
//! through `stylus_sdk::crypto::keccak` rather than through the `Host` trait,
//! so the oracle has to be swapped in with `#[kani::stub]`.
//!
//! Note: this code is illustrative and has not been audited.

#![cfg_attr(not(any(test, feature = "export-abi")), no_main)]
extern crate alloc;

use alloc::vec::Vec;

use stylus_sdk::{
    alloy_primitives::{Address, U256},
    prelude::*,
};

sol_storage! {
    #[entrypoint]
    pub struct Vault {
        address owner;
        mapping(address => uint256) balances;
        uint256 total;
    }
}

#[public]
impl Vault {
    pub fn owner(&self) -> Address {
        self.owner.get()
    }

    pub fn balance_of(&self, who: Address) -> U256 {
        self.balances.get(who)
    }

    pub fn total(&self) -> U256 {
        self.total.get()
    }

    /// One-shot: claim ownership while it is still unset.
    pub fn claim_ownership(&mut self) -> Result<(), Vec<u8>> {
        if self.owner.get() != Address::ZERO {
            return Err(b"already owned".to_vec());
        }
        self.owner.set(self.vm().msg_sender());
        Ok(())
    }

    /// Owner-only. `only_owner_can_transfer_ownership` proves the guard holds
    /// for every possible caller.
    pub fn transfer_ownership(&mut self, new_owner: Address) -> Result<(), Vec<u8>> {
        if self.vm().msg_sender() != self.owner.get() {
            return Err(b"not owner".to_vec());
        }
        self.owner.set(new_owner);
        Ok(())
    }

    /// Credit an account, written the way a lot of real contract code is —
    /// bare `+` on `U256`. `credit_can_silently_wrap` shows what that costs.
    pub fn credit(&mut self, who: Address, amount: U256) {
        let current = self.balances.get(who);
        self.balances.insert(who, current + amount);
        self.total.set(self.total.get() + amount);
    }

    /// The guarded version: rejects instead of wrapping.
    pub fn credit_checked(&mut self, who: Address, amount: U256) -> Result<(), Vec<u8>> {
        let current = self.balances.get(who);
        let next = current.checked_add(amount).ok_or(b"overflow".to_vec())?;
        let total = self
            .total
            .get()
            .checked_add(amount)
            .ok_or(b"overflow".to_vec())?;
        self.balances.insert(who, next);
        self.total.set(total);
        Ok(())
    }
}

/// Ordinary unit tests: one concrete scenario each. Compare with the proofs
/// below, which cover every input at once.
#[cfg(test)]
mod test {
    use super::*;
    use stylus_sdk::testing::*;

    #[test]
    fn test_vault() {
        let vm = TestVM::default();
        let mut vault = Vault::from(&vm);

        let alice = Address::from([1u8; 20]);
        let bob = Address::from([2u8; 20]);

        assert_eq!(U256::ZERO, vault.total());

        vault.credit(alice, U256::from(100));
        vault.credit(bob, U256::from(50));

        assert_eq!(U256::from(100), vault.balance_of(alice));
        assert_eq!(U256::from(50), vault.balance_of(bob));
        assert_eq!(U256::from(150), vault.total());
    }

    #[test]
    fn test_ownership() {
        let vm = TestVM::default();
        let owner = vm.msg_sender();
        let mut vault = Vault::from(&vm);

        vault.claim_ownership().unwrap();
        assert_eq!(owner, vault.owner());

        // Claiming twice fails.
        assert!(vault.claim_ownership().is_err());
    }
}

#[cfg(all(kani, not(feature = "proofs")))]
compile_error!("proofs need the `proofs` feature: cargo kani --features proofs -Z stubbing");

/// Formal proofs, checked by `cargo kani --features proofs -Z stubbing`.
///
/// Compiled only under verification, so `cargo test`, `cargo build` and
/// `cargo stylus check` are unaffected.
#[cfg(kani)]
mod proofs {
    use super::*;
    use kani_stylus_core::{any_address, any_u256, SymbolicVM};

    // -- access control: no mappings, so no stub needed ----------------------

    /// Nobody but the owner can transfer ownership — for *every* caller.
    ///
    /// Note the shape: set up arbitrary pre-state, assume the precondition,
    /// call, then assert both that it failed **and** that state is unchanged.
    /// Checking only the return value would miss a method that errors after
    /// having already written.
    #[kani::proof]
    fn only_owner_can_transfer_ownership() {
        let owner = any_address();
        let attacker = any_address();
        let target = any_address();
        kani::assume(attacker != owner);

        let vm = SymbolicVM::new().with_sender(attacker);
        let mut v = Vault::from(&vm);
        // Writing the field directly establishes arbitrary pre-state: no
        // assumption about how the vault got here.
        v.owner.set(owner);

        let result = v.transfer_ownership(target);

        assert!(result.is_err(), "a non-owner transferred ownership");
        assert_eq!(v.owner(), owner, "ownership changed despite a failed call");
    }

    /// The owner *can* transfer. Without this, the proof above would be
    /// satisfied by a `transfer_ownership` that always fails.
    #[kani::proof]
    fn owner_can_transfer_ownership() {
        let owner = any_address();
        let target = any_address();

        let vm = SymbolicVM::new().with_sender(owner);
        let mut v = Vault::from(&vm);
        v.owner.set(owner);

        assert!(v.transfer_ownership(target).is_ok());
        assert_eq!(v.owner(), target);
    }

    /// `claim_ownership` is one-shot: it cannot be re-run to steal the vault.
    #[kani::proof]
    fn ownership_cannot_be_claimed_twice() {
        let owner = any_address();
        let attacker = any_address();
        kani::assume(owner != Address::ZERO);

        let vm = SymbolicVM::new().with_sender(attacker);
        let mut v = Vault::from(&vm);
        v.owner.set(owner);

        assert!(v.claim_ownership().is_err());
        assert_eq!(v.owner(), owner);
    }

    // -- mappings: these need the keccak stub --------------------------------

    /// A credited balance reads back. Exercises keccak *determinism*: the write
    /// and the read must land on the same slot.
    #[kani::proof]
    #[kani::stub(stylus_sdk::crypto::keccak, kani_stylus_core::keccak_stub)]
    fn credit_then_read_roundtrips() {
        let who = any_address();
        let amount = any_u256();

        let vm = SymbolicVM::new();
        let mut v = Vault::from(&vm);
        v.credit(who, amount);

        assert_eq!(v.balance_of(who), amount);
    }

    /// **Keccak injectivity, made checkable.** Two different accounts must not
    /// share a slot. If the oracle were unsound — a constant, say, or a cheap
    /// non-injective mix — this proof would fail.
    #[kani::proof]
    #[kani::stub(stylus_sdk::crypto::keccak, kani_stylus_core::keccak_stub)]
    fn distinct_accounts_do_not_alias() {
        let a = any_address();
        let b = any_address();
        kani::assume(a != b);

        let x = any_u256();
        let y = any_u256();
        kani::assume(x.checked_add(y).is_some());

        let vm = SymbolicVM::new();
        let mut v = Vault::from(&vm);
        v.credit_checked(a, x).unwrap();
        v.credit_checked(b, y).unwrap();

        assert_eq!(v.balance_of(a), x, "crediting b disturbed a's balance");
        assert_eq!(v.balance_of(b), y, "crediting a disturbed b's balance");
    }

    /// The unguarded `credit` breaks conservation by wrapping — the same class
    /// of bug as the counter's, but here it silently destroys value.
    #[kani::proof]
    #[kani::should_panic]
    #[kani::stub(stylus_sdk::crypto::keccak, kani_stylus_core::keccak_stub)]
    fn credit_can_silently_wrap() {
        let who = any_address();
        let x = any_u256();
        let y = any_u256();

        let vm = SymbolicVM::new();
        let mut v = Vault::from(&vm);
        v.credit(who, x);
        v.credit(who, y);

        assert!(v.balance_of(who) >= x, "crediting reduced a balance");
    }

    /// `credit_checked` rejects instead of wrapping — the fix, proved.
    #[kani::proof]
    #[kani::stub(stylus_sdk::crypto::keccak, kani_stylus_core::keccak_stub)]
    fn credit_checked_never_wraps() {
        let who = any_address();
        let x = any_u256();
        let y = any_u256();

        let vm = SymbolicVM::new();
        let mut v = Vault::from(&vm);

        if v.credit_checked(who, x).is_ok() && v.credit_checked(who, y).is_ok() {
            assert!(v.balance_of(who) >= x);
        }
    }
}
