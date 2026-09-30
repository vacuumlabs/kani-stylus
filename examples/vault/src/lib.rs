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
//!   module for the decomposition and what part of it is a hand argument;
//! - **allowances** — ERC-20's `approve`/`transfer_from`, a nested mapping,
//!   with proofs from an arbitrary starting state. Three mapping entries move
//!   per call, which is where the storage model's cost used to bite; see
//!   `kb/36-storage-model.md`.
//!
//! ```bash
//! cargo test                                      # ordinary unit tests
//! cargo kani --features proofs -Z stubbing        # the proofs
//! ```
//!
//! `-Z stubbing` is required here and not in the counter: harnesses that
//! touch mappings are declared with `kani_stylus_core::proof!`, which stubs
//! the SDK's slot derivation so mapping entries get structured slots instead
//! of keccak digests. Add `kani-stylus-core/precise-storage` to the features
//! and the same harnesses run the SDK's real derivation through the keccak
//! oracle instead.
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
        mapping(address => mapping(address => uint256)) allowances;
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

    pub fn allowance(&self, owner: Address, spender: Address) -> U256 {
        self.allowances.getter(owner).get(spender)
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
        self.move_balance(from, to, amount)
    }

    /// Let `spender` move up to `amount` of the caller's balance.
    pub fn approve(&mut self, spender: Address, amount: U256) {
        let owner = self.vm().msg_sender();
        self.allowances.setter(owner).insert(spender, amount);
    }

    /// Move `amount` from `from` to `to`, spending the caller's allowance.
    /// The ERC-20 `transferFrom` shape: three mapping entries change, one of
    /// them in a nested map.
    pub fn transfer_from(&mut self, from: Address, to: Address, amount: U256) -> Result<(), Vec<u8>> {
        let spender = self.vm().msg_sender();
        let allowed = self.allowances.getter(from).get(spender);
        if allowed < amount {
            return Err(b"insufficient allowance".to_vec());
        }
        self.move_balance(from, to, amount)?;
        self.allowances.setter(from).insert(spender, allowed - amount);
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

impl Vault {
    /// The part `transfer` and `transfer_from` share. Checked throughout, and
    /// self-transfer is rejected: see `transfer`.
    fn move_balance(&mut self, from: Address, to: Address, amount: U256) -> Result<(), Vec<u8>> {
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
    fn test_transfer_from() {
        let vm = TestVM::default();
        let alice = vm.msg_sender();
        let bob = Address::from([2u8; 20]);
        let carol = Address::from([3u8; 20]);
        let mut vault = Vault::from(&vm);

        vault.credit(alice, U256::from(100));
        vault.approve(bob, U256::from(40));
        assert_eq!(U256::from(40), vault.allowance(alice, bob));

        vm.set_sender(bob);
        vault.transfer_from(alice, carol, U256::from(30)).unwrap();
        assert_eq!(U256::from(70), vault.balance_of(alice));
        assert_eq!(U256::from(30), vault.balance_of(carol));
        assert_eq!(U256::from(10), vault.allowance(alice, bob));
        assert_eq!(U256::from(100), vault.total());

        // Past the allowance, and nothing moves.
        assert!(vault.transfer_from(alice, carol, U256::from(20)).is_err());
        assert_eq!(U256::from(70), vault.balance_of(alice));
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

    // -- mappings ------------------------------------------------------------
    //
    // Everything that touches a mapping is declared with `proof!`, which turns
    // each function into a `#[kani::proof]` and attaches the stubs for mapping
    // slots: structured slots by default, the SDK's own keccak derivation under
    // `kani-stylus-core/precise-storage`. The harnesses are the same either
    // way; see `kb/36-storage-model.md` for what each mode assumes and costs.

    kani_stylus_core::proof! {
        /// A credited balance reads back. Exercises slot *determinism*: the
        /// write and the read must land on the same slot.
        fn credit_then_read_roundtrips() {
            let who = any_address();
            let amount = any_u256();

            let vm = SymbolicVM::new();
            let mut v = Vault::from(&vm);
            v.credit(who, amount);

            assert_eq!(v.balance_of(who), amount);
        }

        /// **Slot injectivity, made checkable.** Two different accounts must
        /// not share a slot. If the slot model were unsound — a constant, say,
        /// or a cheap non-injective mix — this proof would fail.
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

        /// The unguarded `credit` breaks conservation by wrapping — the same
        /// class of bug as the counter's, but here it silently destroys value.
        #[kani::should_panic]
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
    // The arbitrary pre-state is `with_arbitrary_storage()`: every slot the
    // proof reads starts out as an arbitrary value, so no assumption is made
    // about how the vault got here, and nothing has to be seeded by hand. Each
    // lemma carries a `kani::cover` showing the interesting case is reachable,
    // so a bound that is too tight fails loudly instead of passing vacuously.

    kani_stylus_core::proof! {
        /// **Lemma 1: `credit_checked` moves `total` and the balance in step.**
        ///
        /// Mint-shaped: supply rises by exactly what the balance rises by.
        fn credit_checked_moves_total_by_the_same_delta() {
            let who = any_address();
            let amount = any_u256();

            let vm = SymbolicVM::concrete_ctx().with_arbitrary_storage();
            let mut v = Vault::from(&vm);

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
                kani::cover!(vm.slots_changed_since(&before_storage) == 2, "both slots move");
            }
        }

        /// **Lemma 2: `transfer` leaves the supply alone.**
        ///
        /// The ERC-20 half. `credit` raises `total`; `transfer` must not touch
        /// it, while moving exactly `amount` between two balances.
        fn transfer_conserves_total() {
            let from = any_address();
            let to = any_address();
            let amount = any_u256();

            let vm = SymbolicVM::concrete_ctx().with_sender(from).with_arbitrary_storage();
            let mut v = Vault::from(&vm);

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
                kani::cover!(vm.slots_changed_since(&before_storage) == 2, "both balances move");
            }
        }

        /// **The frame condition in its strongest form: no *other* address
        /// moves.**
        ///
        /// `other` is symbolic and assumed distinct from both parties, so
        /// `unsat` covers every remaining address at once — the universal
        /// quantifier a bounded model checker gives you for free, by proving
        /// no counterexample exists. Strictly stronger than the slot-count
        /// frame above, and it costs a third mapping account; both forms are
        /// kept so the trade-off is visible.
        fn transfer_does_not_move_any_other_balance() {
            let from = any_address();
            let to = any_address();
            let other = any_address();
            kani::assume(other != from && other != to);
            let amount = any_u256();

            let vm = SymbolicVM::concrete_ctx().with_sender(from).with_arbitrary_storage();
            let mut v = Vault::from(&vm);

            let before_other = v.balance_of(other);

            if v.transfer(to, amount).is_ok() {
                assert_eq!(
                    v.balance_of(other),
                    before_other,
                    "an uninvolved account's balance moved"
                );
            }
        }

        /// **Lemma 3: `approve` sets exactly one allowance.**
        fn approve_sets_exactly_one_allowance() {
            let owner = any_address();
            let spender = any_address();
            let amount = any_u256();

            let vm = SymbolicVM::concrete_ctx().with_sender(owner).with_arbitrary_storage();
            let mut v = Vault::from(&vm);
            let before_storage = vm.snapshot();

            v.approve(spender, amount);

            assert_eq!(v.allowance(owner, spender), amount);
            assert!(vm.slots_changed_since(&before_storage) <= 1, "approve wrote a second slot");
        }

        /// **Lemma 4: `transfer_from` conserves supply.**
        ///
        /// Lemma 2's statement for the delegated path, from an arbitrary
        /// state and for every caller.
        fn transfer_from_conserves_total() {
            let spender = any_address();
            let from = any_address();
            let to = any_address();
            let amount = any_u256();

            let vm = SymbolicVM::concrete_ctx().with_sender(spender).with_arbitrary_storage();
            let mut v = Vault::from(&vm);

            let before_from = v.balance_of(from);
            let before_to = v.balance_of(to);
            let before_total = v.total();

            if v.transfer_from(from, to, amount).is_ok() {
                assert_eq!(v.total(), before_total, "transfer_from changed the supply");
                assert_eq!(
                    v.balance_of(from),
                    before_from.checked_sub(amount).unwrap(),
                    "owner did not lose exactly `amount`"
                );
                assert_eq!(
                    v.balance_of(to),
                    before_to.checked_add(amount).unwrap(),
                    "recipient did not gain exactly `amount`"
                );
            }
        }

        /// **Lemma 5: `transfer_from` spends exactly the allowance it uses,
        /// and writes three slots at most** — two balances and one allowance
        /// in a nested map.
        ///
        /// Split from lemma 4 on purpose: with all four identities in one
        /// harness the solver ran past 15 minutes.
        fn transfer_from_spends_exactly_the_allowance() {
            let spender = any_address();
            let from = any_address();
            let to = any_address();
            let amount = any_u256();

            let vm = SymbolicVM::concrete_ctx().with_sender(spender).with_arbitrary_storage();
            let mut v = Vault::from(&vm);

            let before_allowance = v.allowance(from, spender);
            let before_storage = vm.snapshot();

            if v.transfer_from(from, to, amount).is_ok() {
                assert_eq!(
                    v.allowance(from, spender),
                    before_allowance.checked_sub(amount).unwrap(),
                    "the allowance did not drop by exactly `amount`"
                );
                assert!(
                    vm.slots_changed_since(&before_storage) <= 3,
                    "transfer_from wrote a fourth slot"
                );
                kani::cover!(vm.slots_changed_since(&before_storage) == 3, "all three entries move");
            }
        }

        /// **`transfer_from` moves no other balance and no other allowance.**
        ///
        /// Symbolic third parties again, one per map: every other account, and
        /// every other `(owner, spender)` pair.
        fn transfer_from_moves_nothing_else() {
            let spender = any_address();
            let from = any_address();
            let to = any_address();
            let amount = any_u256();
            let other = any_address();
            kani::assume(other != from && other != to);
            let (owner2, spender2) = (any_address(), any_address());
            kani::assume(owner2 != from || spender2 != spender);

            let vm = SymbolicVM::concrete_ctx().with_sender(spender).with_arbitrary_storage();
            let mut v = Vault::from(&vm);

            let before_other = v.balance_of(other);
            let before_allowance = v.allowance(owner2, spender2);

            if v.transfer_from(from, to, amount).is_ok() {
                assert_eq!(v.balance_of(other), before_other, "an uninvolved balance moved");
                assert_eq!(
                    v.allowance(owner2, spender2),
                    before_allowance,
                    "an uninvolved allowance moved"
                );
            }
        }
    }
}
