//! A small vault: an owner, per-account balances, and a running total.
//!
//! Two things the counter can't show:
//!
//! - **access control** over a symbolic caller — the `msg_sender` case;
//! - **mappings**, which are keccak-derived storage slots, and so exercise the
//!   hash oracle in `kani_stylus_core::keccak`.
//!
//! Every harness touching a mapping needs the keccak stub and `-Z stubbing`.
//! See the note on [`proofs`].

use stylus_sdk::{
    alloy_primitives::{Address, U256},
    prelude::*,
};

sol_storage! {
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

    /// One-shot: claim ownership while it is unset.
    pub fn claim_ownership(&mut self) -> Result<(), Vec<u8>> {
        if self.owner.get() != Address::ZERO {
            return Err(b"already owned".to_vec());
        }
        self.owner.set(self.vm().msg_sender());
        Ok(())
    }

    /// Owner-only. The property `only_owner_can_transfer_ownership` proves the
    /// guard actually holds for every possible caller.
    pub fn transfer_ownership(&mut self, new_owner: Address) -> Result<(), Vec<u8>> {
        if self.vm().msg_sender() != self.owner.get() {
            return Err(b"not owner".to_vec());
        }
        self.owner.set(new_owner);
        Ok(())
    }

    /// Credit an account. Deliberately written the way a lot of real contract
    /// code is — bare `+` on `U256`. `credit_can_silently_wrap` shows what
    /// that costs.
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

#[cfg(kani)]
mod proofs {
    use super::*;
    use kani_stylus_core::{any_address, any_u256, SymbolicVM};

    // ---- access control: no mapping, so no stub needed ---------------------

    /// Nobody but the owner can transfer ownership — for *every* caller
    /// address, not a handful of test accounts.
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

        let vm = SymbolicVM::concrete_ctx().with_sender(attacker);
        let mut v = Vault::from(&vm);
        // Writing the field directly is how you establish arbitrary pre-state:
        // no constructor call, no assumption about how the vault got here.
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

        let vm = SymbolicVM::concrete_ctx().with_sender(owner);
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

        let vm = SymbolicVM::concrete_ctx().with_sender(attacker);
        let mut v = Vault::from(&vm);
        v.owner.set(owner);

        assert!(v.claim_ownership().is_err());
        assert_eq!(v.owner(), owner);
    }

    // ---- mappings: these need the keccak stub ------------------------------
    //
    // Stylus mappings hash through `stylus_sdk::crypto::keccak`, not through
    // the `Host` trait, so the stub attribute is required. Run these with:
    //
    //     cargo kani -Z stubbing

    /// A credited balance reads back. Exercises keccak determinism: the write
    /// and the read must land on the same slot.
    #[kani::proof]
    #[kani::stub(stylus_sdk::crypto::keccak, kani_stylus_core::keccak_stub)]
    fn credit_then_read_roundtrips() {
        let who = any_address();
        let amount = any_u256();

        let vm = SymbolicVM::concrete_ctx();
        let mut v = Vault::from(&vm);
        v.credit(who, amount);

        assert_eq!(v.balance_of(who), amount);
    }

    /// **Keccak injectivity, made checkable.** Two different accounts must not
    /// share a slot. If the hash oracle were unsound — a constant, say, or a
    /// cheap non-injective mix — this proof would fail.
    #[kani::proof]
    #[kani::stub(stylus_sdk::crypto::keccak, kani_stylus_core::keccak_stub)]
    fn distinct_accounts_do_not_alias() {
        let a = any_address();
        let b = any_address();
        kani::assume(a != b);

        let x = any_u256();
        let y = any_u256();
        kani::assume(x.checked_add(y).is_some());

        let vm = SymbolicVM::concrete_ctx();
        let mut v = Vault::from(&vm);
        v.credit_checked(a, x).unwrap();
        v.credit_checked(b, y).unwrap();

        assert_eq!(v.balance_of(a), x, "crediting b disturbed a's balance");
        assert_eq!(v.balance_of(b), y, "crediting a disturbed b's balance");
    }

    /// Conservation: the total equals the sum of what was credited.
    #[kani::proof]
    #[kani::stub(stylus_sdk::crypto::keccak, kani_stylus_core::keccak_stub)]
    fn total_tracks_the_sum_of_balances() {
        let a = any_address();
        let b = any_address();
        kani::assume(a != b);

        let x = any_u256();
        let y = any_u256();
        kani::assume(x.checked_add(y).is_some());

        let vm = SymbolicVM::concrete_ctx();
        let mut v = Vault::from(&vm);
        v.credit_checked(a, x).unwrap();
        v.credit_checked(b, y).unwrap();

        assert_eq!(v.total(), x + y);
        assert_eq!(v.balance_of(a) + v.balance_of(b), v.total());
    }

    /// The unguarded `credit` breaks conservation by wrapping. Same class of
    /// bug as the counter's, but here it silently mints or destroys value.
    #[kani::proof]
    #[kani::should_panic]
    #[kani::stub(stylus_sdk::crypto::keccak, kani_stylus_core::keccak_stub)]
    fn credit_can_silently_wrap() {
        let who = any_address();
        let x = any_u256();
        let y = any_u256();

        let vm = SymbolicVM::concrete_ctx();
        let mut v = Vault::from(&vm);
        v.credit(who, x);
        v.credit(who, y);

        assert!(
            v.balance_of(who) >= x,
            "crediting reduced an account's balance"
        );
    }

    /// `credit_checked` rejects instead of wrapping — the fix, proved.
    #[kani::proof]
    #[kani::stub(stylus_sdk::crypto::keccak, kani_stylus_core::keccak_stub)]
    fn credit_checked_never_wraps() {
        let who = any_address();
        let x = any_u256();
        let y = any_u256();

        let vm = SymbolicVM::concrete_ctx();
        let mut v = Vault::from(&vm);

        if v.credit_checked(who, x).is_ok() && v.credit_checked(who, y).is_ok() {
            assert!(v.balance_of(who) >= x);
        }
    }
}
