//! A small vault, verified with Kani.
//!
//! This is an ordinary Stylus contract — `cargo stylus new vault`, with the
//! contract body written and proofs added in place. It covers three things the
//! [counter example](../../counter) has no occasion for:
//!
//! - **access control** over a symbolic caller, so `only_owner_can_transfer`
//!   holds for *every* address rather than a handful of test accounts;
//! - **mappings**, whose slots are keccak-derived, which exercises the hash
//!   oracle in `kani_stylus_core::keccak`;
//! - **conservation** — `total` tracking the sum of all balances — proved by
//!   local per-method lemmas rather than by summing balances, which a bounded
//!   model checker cannot do. See the `-- conservation` block in the proofs
//!   module for the decomposition and what part of it is a hand argument.
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

    /// Move `amount` from the caller to `to`. The ERC-20 shape: unlike
    /// `credit`, this must leave `total` **untouched** — which is exactly what
    /// the conservation lemmas below check.
    ///
    /// Checked throughout, and self-transfer is rejected rather than handled:
    /// `from == to` with naive read-modify-write is a classic way to mint
    /// tokens out of nothing, and refusing it is simpler than getting the
    /// ordering right.
    pub fn transfer(&mut self, to: Address, amount: U256) -> Result<(), Vec<u8>> {
        let from = self.vm().msg_sender();
        if from == to {
            return Err(b"self transfer".to_vec());
        }
        let from_balance = self.balances.get(from);
        if from_balance < amount {
            return Err(b"insufficient balance".to_vec());
        }
        let to_balance = self
            .balances
            .get(to)
            .checked_add(amount)
            .ok_or(b"overflow".to_vec())?;
        self.balances.insert(from, from_balance - amount);
        self.balances.insert(to, to_balance);
        Ok(())
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
    fn test_transfer() {
        let vm = TestVM::default();
        let from = vm.msg_sender();
        let bob = Address::from([2u8; 20]);
        let mut vault = Vault::from(&vm);

        vault.credit(from, U256::from(100));
        vault.transfer(bob, U256::from(30)).unwrap();

        assert_eq!(U256::from(70), vault.balance_of(from));
        assert_eq!(U256::from(30), vault.balance_of(bob));
        // The point of `transfer`: supply is untouched.
        assert_eq!(U256::from(100), vault.total());

        assert!(vault.transfer(bob, U256::from(1000)).is_err());
        assert!(vault.transfer(from, U256::from(1)).is_err());
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
    use kani_stylus_core::{any_address, any_u256, SymbolicVm, SymbolicVM};

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

    // -- conservation, by local deltas --------------------------------------
    //
    // The property we actually want is `total == sum of every balance`. A
    // bounded model checker cannot state it: there is no quantifying over
    // 2^160 addresses, and summing N of them explicitly did not converge (see
    // `kb/50-feasibility.md`).
    //
    // So it is decomposed. For each method, prove *locally* that
    //
    //   (a) it moves `total` by exactly the net amount it moves balances by,
    //   (b) it changes nothing else  — the *frame condition*,
    //
    // both from an **arbitrary** pre-state. Global conservation then follows by
    // induction over any call sequence: if the sum invariant held before a
    // call, (a) and (b) say it holds after.
    //
    // **That last step is a hand argument, not machine-checked.** Kani proves
    // the per-method lemmas; the induction over sequences is on paper. This is
    // the standard decomposition — it is what Certora rules do for Solidity,
    // and nobody sums 2^160 balances — but it has to be said out loud rather
    // than implied.
    //
    // `SymbolicVm::<4>` rather than the default 16: these proofs touch two or
    // three slots, and both the slot scan and `changed_since` cost grow with
    // the bound. Each lemma carries a `kani::cover` proving the expected number
    // of slots is actually reachable, so a too-tight bound fails loudly
    // instead of passing vacuously.

    /// **Lemma 1: `credit_checked` moves `total` and the balance in step.**
    ///
    /// Mint-shaped: supply rises by exactly what the balance rises by.
    #[kani::proof]
    #[kani::stub(stylus_sdk::crypto::keccak, kani_stylus_core::keccak_stub)]
    fn credit_checked_moves_total_by_the_same_delta() {
        let who = any_address();
        let amount = any_u256();

        let vm = SymbolicVm::<4>::concrete_ctx();
        let mut v = Vault::from(&vm);

        // Arbitrary pre-state: no assumption about how the vault got here.
        v.balances.insert(who, any_u256());
        v.total.set(any_u256());

        let before_balance = v.balance_of(who);
        let before_total = v.total();
        let before_storage = vm.snapshot();

        if v.credit_checked(who, amount).is_ok() {
            // (a) the two deltas agree. `checked_*` in the assertion too:
            // bare `+` on U256 wraps, so it would weaken what is proved.
            assert_eq!(
                v.balance_of(who),
                before_balance.checked_add(amount).unwrap(),
                "balance moved by something other than `amount`"
            );
            assert_eq!(
                v.total(),
                before_total.checked_add(amount).unwrap(),
                "total moved by something other than `amount`"
            );
            // (b) frame: the balance slot and the total slot, nothing more.
            assert!(
                vm.slots_changed_since(&before_storage) <= 2,
                "credit_checked wrote a slot it has no business writing"
            );
            kani::cover!(vm.slots_touched() == 2, "both slots reachable");
        }
    }

    /// **Lemma 2: `transfer` leaves the supply alone.**
    ///
    /// The ERC-20 half. `credit` raises `total`; `transfer` must not touch it,
    /// while moving exactly `amount` between two balances.
    #[kani::proof]
    #[kani::stub(stylus_sdk::crypto::keccak, kani_stylus_core::keccak_stub)]
    fn transfer_conserves_total() {
        let from = any_address();
        let to = any_address();
        let amount = any_u256();

        let vm = SymbolicVm::<4>::concrete_ctx().with_sender(from);
        let mut v = Vault::from(&vm);

        v.balances.insert(from, any_u256());
        v.balances.insert(to, any_u256());
        v.total.set(any_u256());

        let before_from = v.balance_of(from);
        let before_to = v.balance_of(to);
        let before_total = v.total();
        let before_storage = vm.snapshot();

        if v.transfer(to, amount).is_ok() {
            assert_eq!(v.total(), before_total, "transfer changed the supply");
            assert_eq!(
                v.balance_of(from),
                before_from.checked_sub(amount).unwrap(),
                "sender did not lose exactly `amount`"
            );
            assert_eq!(
                v.balance_of(to),
                before_to.checked_add(amount).unwrap(),
                "recipient did not gain exactly `amount`"
            );
            // Frame: two balance slots moved; `total`'s slot did not.
            assert!(
                vm.slots_changed_since(&before_storage) <= 2,
                "transfer wrote a third slot"
            );
            kani::cover!(vm.slots_touched() == 3, "all three slots reachable");
        }
    }

    /// **The frame condition in its strongest form: no *other* address moves.**
    ///
    /// `other` is symbolic and assumed distinct from both parties, so `unsat`
    /// covers every remaining address at once — the universal quantifier a
    /// bounded model checker gives you for free, by proving no counterexample
    /// exists. Strictly stronger than the slot-count frame above, and it costs
    /// a third mapping account; both forms are kept so the trade-off is visible.
    #[kani::proof]
    #[kani::stub(stylus_sdk::crypto::keccak, kani_stylus_core::keccak_stub)]
    fn transfer_does_not_move_any_other_balance() {
        let from = any_address();
        let to = any_address();
        let other = any_address();
        kani::assume(other != from && other != to);
        let amount = any_u256();

        let vm = SymbolicVm::<4>::concrete_ctx().with_sender(from);
        let mut v = Vault::from(&vm);

        v.balances.insert(from, any_u256());
        v.balances.insert(to, any_u256());
        v.balances.insert(other, any_u256());

        let before_other = v.balance_of(other);

        if v.transfer(to, amount).is_ok() {
            assert_eq!(
                v.balance_of(other),
                before_other,
                "an uninvolved account's balance moved"
            );
        }
    }
}
