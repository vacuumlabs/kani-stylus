//! `SymbolicVM` — an implementation of `stylus_core::Host` backed by symbolic
//! values, so Kani can verify a Stylus contract over all inputs at once.

use alloc::vec::Vec;

use alloy_primitives::{Address, B256, U256};
use stylus_sdk::stylus_core::*;

use crate::context::Context;
use crate::keccak;
use crate::slots;
use crate::storage::{with_store, Store, MAX_SLOTS};

/// The default VM: up to 16 large storage slots — mapping entries and other
/// hashed slots. A contract's own fields do not count against it.
///
/// Need more? Name the parameter: `SymbolicVm::<32>::new()`. The keccak and
/// mapping-entry bounds are separate and global — see
/// [`crate::keccak::MAX_HASHES`] and [`crate::slots::MAX_ENTRIES`].
pub type SymbolicVM = SymbolicVm<16>;

/// A symbolic Stylus host.
///
/// `SLOTS` bounds the distinct *large* storage slots one proof may touch:
/// mapping entries and other hashed slots. Slots below
/// [`SMALL_SLOTS`](crate::storage::SMALL_SLOTS), which is where a contract's
/// own fields live, are free. Exceeding the bound fails the proof loudly.
///
/// # One VM per proof
///
/// The host is zero-sized: storage and context are globals, like the keccak
/// and arithmetic oracles. That is what makes it cheap — the SDK clones its
/// `Box<dyn Host>` on every storage access, and cloning a zero-sized box
/// allocates nothing. Measured on four fields read back, the formula went
/// from 577k to 221k variables. See `kb/36-storage-model.md`.
///
/// So there is one chain state per proof. Building a second VM with
/// [`new`](Self::new) or its siblings fails the proof; handles made by
/// cloning, or by [`with_timestamp`](Self::with_timestamp), share it.
/// Changing the context changes it for every handle — which is the next
/// transaction, as it is on chain.
///
/// # Example
///
/// ```ignore
/// #[kani::proof]
/// fn transfer_conserves_supply() {
///     let vm = SymbolicVM::new();
///     let mut token = Token::from(&vm);
///     // ... symbolic inputs, then assert the invariant
/// }
/// ```
#[derive(Clone, Copy)]
pub struct SymbolicVm<const SLOTS: usize>;

/// The one context. See [`SymbolicVm`] and [`crate::context`].
static mut CTX: Context = Context::concrete();
static mut BUILT: bool = false;

fn with_ctx<R>(f: impl FnOnce(&mut Context) -> R) -> R {
    // SAFETY: Kani gives each harness its own program and verifies sequential
    // code only, as for the store and the oracles.
    unsafe { f(&mut *core::ptr::addr_of_mut!(CTX)) }
}

impl<const SLOTS: usize> SymbolicVm<SLOTS> {
    const FITS: () = assert!(SLOTS <= MAX_SLOTS, "SymbolicVm::<SLOTS> exceeds storage::MAX_SLOTS");

    /// Symbolic storage and a fully symbolic transaction context.
    pub fn new() -> Self {
        Self::with_context(Context::symbolic())
    }

    /// Symbolic storage, concrete transaction context. Cheaper; use when the
    /// property doesn't depend on sender, value, or block.
    pub fn concrete_ctx() -> Self {
        Self::with_context(Context::concrete())
    }

    pub fn with_context(ctx: Context) -> Self {
        #[allow(clippy::let_unit_value)]
        let () = Self::FITS;
        // SAFETY: see `with_ctx`.
        unsafe {
            assert!(
                !*core::ptr::addr_of!(BUILT),
                "kani-stylus: one SymbolicVm per proof -- clone it, or use with_timestamp/with_sender"
            );
            *core::ptr::addr_of_mut!(BUILT) = true;
        }
        with_ctx(|c| *c = ctx);
        Self
    }

    /// Unwritten storage reads as an **arbitrary** value, fixed per slot,
    /// rather than zero — so a proof starts from every possible state of the
    /// contract at once instead of a fresh deployment.
    ///
    /// ```ignore
    /// let vm = SymbolicVM::concrete_ctx().with_arbitrary_storage();
    /// let mut v = Vault::from(&vm);
    /// // v.balance_of(a), v.total(), ... are all arbitrary, and consistent
    /// ```
    ///
    /// This is the pre-state of an inductive step, and it replaces seeding
    /// each field by hand. It includes states no sequence of calls reaches —
    /// `total` below the sum of balances, say — so state the invariant the
    /// property needs with `kani::assume`, or a counterexample may start
    /// somewhere unreachable.
    ///
    /// With it, *reading* a large slot also uses one of the `SLOTS`, since the
    /// value drawn has to be remembered. Call it before any storage access.
    pub fn with_arbitrary_storage(self) -> Self {
        with_store(|s| s.make_arbitrary());
        self
    }

    /// Set the caller, for this call and every later one.
    ///
    /// ```ignore
    /// let vm = SymbolicVM::concrete_ctx().with_sender(owner);
    /// ```
    pub fn with_sender(self, sender: Address) -> Self {
        with_ctx(|c| c.msg_sender = sender);
        self
    }

    pub fn with_value(self, value: U256) -> Self {
        with_ctx(|c| c.msg_value = value);
        self
    }

    /// Advance the clock: a handle on the same storage, at `block_timestamp`.
    ///
    /// Context is shared by every handle, so this is the clock for all of
    /// them from here on — the next transaction. It borrows rather than
    /// consuming so the old handle stays usable.
    ///
    /// ```ignore
    /// let mut v1 = Contract::from(&vm);
    /// v1.claim();                       // at vm's timestamp
    /// let vm2 = vm.with_timestamp(t2);
    /// let mut v2 = Contract::from(&vm2);
    /// v2.claim();                       // sees v1's writes, at t2
    /// ```
    pub fn with_timestamp(&self, block_timestamp: u64) -> Self {
        with_ctx(|c| c.block_timestamp = block_timestamp);
        Self
    }

    pub fn context(&self) -> Context {
        with_ctx(|c| *c)
    }

    /// Distinct storage slots written so far (with arbitrary storage, also
    /// large slots read).
    ///
    /// Use it to confirm a proof isn't silently bounded:
    /// `kani::cover!(vm.slots_touched() == 3)`.
    pub fn slots_touched(&self) -> usize {
        with_store(|s| s.touched())
    }

    /// Distinct keccak256 preimages hashed so far in this proof.
    pub fn hashes_taken(&self) -> usize {
        keccak::hashes_taken()
    }

    /// Distinct mapping entries given a structured slot so far — see
    /// [`crate::slots`]. Zero when the `to_slot` stubs are not in use.
    pub fn entries_derived(&self) -> usize {
        slots::entries_derived()
    }

    /// Freeze the current storage, for use with [`Self::slots_changed_since`].
    ///
    /// This is how you state a **frame condition** — "the call changed exactly
    /// these slots and nothing else" — which is the half of a conservation
    /// argument that asserting deltas alone does not give you.
    ///
    /// ```ignore
    /// let before = vm.snapshot();
    /// token.transfer(to, amount).unwrap();
    /// // `<=`, not `==`: a frame condition is an upper bound on what moved,
    /// // and a zero-amount transfer changes nothing at all.
    /// assert!(vm.slots_changed_since(&before) <= 2, "transfer wrote a third slot");
    /// ```
    ///
    /// Note this counts *slots*, not accounts, and it can only see slots the
    /// proof actually touched. For the stronger statement — "no **other
    /// address**'s balance moved" — read a third symbolic address assumed
    /// distinct from the others and assert its balance is unchanged; a symbolic
    /// address covers every address at once. That is stronger but costs another
    /// mapping access. See `examples/vault` for both forms side by side.
    pub fn snapshot(&self) -> StorageSnapshot<SLOTS> {
        StorageSnapshot(with_store(|s| *s))
    }

    /// Number of storage slots whose value differs from `before`.
    pub fn slots_changed_since(&self, before: &StorageSnapshot<SLOTS>) -> usize {
        with_store(|s| s.changed_since::<SLOTS>(&before.0))
    }
}

/// Storage frozen at a point in time. Produced by [`SymbolicVm::snapshot`].
pub struct StorageSnapshot<const SLOTS: usize>(Store);

impl<const SLOTS: usize> Default for SymbolicVm<SLOTS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const SLOTS: usize> Host for SymbolicVm<SLOTS> {}

// --- the parts a proof actually exercises ----------------------------------

impl<const SLOTS: usize> StorageAccess for SymbolicVm<SLOTS> {
    fn storage_load_bytes32(&self, key: U256) -> B256 {
        with_store(|s| s.load::<SLOTS>(key))
    }
    unsafe fn storage_cache_bytes32(&self, key: U256, value: B256) {
        with_store(|s| s.store::<SLOTS>(key, value))
    }
    // Writes are visible immediately, so flushing is a no-op. Contracts must
    // not be able to observe the difference; if one can, that is a finding.
    fn flush_cache(&self, _clear: bool) {}
}

impl<const SLOTS: usize> CryptographyAccess for SymbolicVm<SLOTS> {
    fn native_keccak256(&self, input: &[u8]) -> B256 {
        // Shared with `keccak_stub`, so a contract hashing explicitly and a
        // mapping hashing internally agree on digests.
        keccak::oracle_hash(input)
    }
}

impl<const SLOTS: usize> MessageAccess for SymbolicVm<SLOTS> {
    fn msg_sender(&self) -> Address {
        with_ctx(|c| c.msg_sender)
    }
    fn msg_value(&self) -> U256 {
        with_ctx(|c| c.msg_value)
    }
    fn msg_reentrant(&self) -> bool {
        false
    }
    fn tx_origin(&self) -> Address {
        with_ctx(|c| c.tx_origin)
    }
}

impl<const SLOTS: usize> BlockAccess for SymbolicVm<SLOTS> {
    fn block_basefee(&self) -> U256 {
        U256::ZERO
    }
    fn block_coinbase(&self) -> Address {
        Address::ZERO
    }
    fn block_number(&self) -> u64 {
        with_ctx(|c| c.block_number)
    }
    fn block_timestamp(&self) -> u64 {
        with_ctx(|c| c.block_timestamp)
    }
    fn block_gas_limit(&self) -> u64 {
        30_000_000
    }
}

impl<const SLOTS: usize> ChainAccess for SymbolicVm<SLOTS> {
    fn chain_id(&self) -> u64 {
        with_ctx(|c| c.chain_id)
    }
}

impl<const SLOTS: usize> AccountAccess for SymbolicVm<SLOTS> {
    fn balance(&self, _account: Address) -> U256 {
        U256::ZERO
    }
    fn contract_address(&self) -> Address {
        with_ctx(|c| c.contract_address)
    }
    fn code(&self, _account: Address) -> Vec<u8> {
        Vec::new()
    }
    fn code_size(&self, _account: Address) -> usize {
        0
    }
    fn code_hash(&self, _account: Address) -> B256 {
        B256::ZERO
    }
}

// --- not modelled: cheap, inert answers ------------------------------------

impl<const SLOTS: usize> CalldataAccess for SymbolicVm<SLOTS> {
    // Proofs call contract methods directly rather than through the ABI router,
    // so calldata is empty. Verifying the router over symbolic calldata is a
    // separate, harder problem — see the crate docs.
    fn read_args(&self, _len: usize) -> Vec<u8> {
        Vec::new()
    }
    fn read_return_data(&self, _offset: usize, _size: Option<usize>) -> Vec<u8> {
        Vec::new()
    }
    fn return_data_size(&self) -> usize {
        0
    }
    fn write_result(&self, _data: &[u8]) {}
}

impl<const SLOTS: usize> MeteringAccess for SymbolicVm<SLOTS> {
    // Gas is not modelled: proofs are about functional correctness, and an
    // out-of-gas path would only add spurious failures.
    fn evm_gas_left(&self) -> u64 {
        u64::MAX
    }
    fn evm_ink_left(&self) -> u64 {
        u64::MAX
    }
    fn tx_gas_price(&self) -> U256 {
        U256::ZERO
    }
    fn tx_ink_price(&self) -> u32 {
        1
    }
}

impl<const SLOTS: usize> MemoryAccess for SymbolicVm<SLOTS> {
    fn pay_for_memory_grow(&self, _pages: u16) {}
}

impl<const SLOTS: usize> RawLogAccess for SymbolicVm<SLOTS> {
    fn emit_log(&self, _input: &[u8], _num_topics: usize) {}
    fn raw_log(&self, _topics: &[B256], _data: &[u8]) -> Result<(), &'static str> {
        Ok(())
    }
}

// --- out of scope: reaching these fails the proof, deliberately -------------
//
// A contract that deploys or calls out is outside what this VM models. Rather
// than invent a return value and quietly verify something untrue, these panic
// so the proof fails and says why.

unsafe impl<const SLOTS: usize> UnsafeDeploymentAccess
    for SymbolicVm<SLOTS>
{
    unsafe fn create1(
        &self,
        _code: *const u8,
        _code_len: usize,
        _endowment: *const u8,
        _contract: *mut u8,
        _revert_data_len: *mut usize,
    ) {
        unimplemented!("kani-stylus: contract deployment (CREATE) is not modelled")
    }
    unsafe fn create2(
        &self,
        _code: *const u8,
        _code_len: usize,
        _endowment: *const u8,
        _salt: *const u8,
        _contract: *mut u8,
        _revert_data_len: *mut usize,
    ) {
        unimplemented!("kani-stylus: contract deployment (CREATE2) is not modelled")
    }
}

unsafe impl<const SLOTS: usize> UnsafeCallAccess
    for SymbolicVm<SLOTS>
{
    unsafe fn call_contract(
        &self,
        _to: *const u8,
        _data: *const u8,
        _data_len: usize,
        _value: *const u8,
        _gas: u64,
        _outs_len: &mut usize,
    ) -> u8 {
        unimplemented!("kani-stylus: cross-contract CALL is not modelled")
    }
    unsafe fn static_call_contract(
        &self,
        _to: *const u8,
        _data: *const u8,
        _data_len: usize,
        _gas: u64,
        _outs_len: &mut usize,
    ) -> u8 {
        unimplemented!("kani-stylus: cross-contract STATICCALL is not modelled")
    }
    unsafe fn delegate_call_contract(
        &self,
        _to: *const u8,
        _data: *const u8,
        _data_len: usize,
        _gas: u64,
        _outs_len: &mut usize,
    ) -> u8 {
        unimplemented!("kani-stylus: cross-contract DELEGATECALL is not modelled")
    }
}
