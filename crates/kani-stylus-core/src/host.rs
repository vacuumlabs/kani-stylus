//! `SymbolicVM` — an implementation of `stylus_core::Host` backed by symbolic
//! values, so Kani can verify a Stylus contract over all inputs at once.

use alloc::vec::Vec;
use core::cell::RefCell;

use alloy_primitives::{Address, B256, U256};
use stylus_sdk::stylus_core::*;

use crate::context::Context;
use crate::keccak;
use crate::storage::SlotStore;

/// The default VM: 16 storage slots. Suits most contracts.
///
/// Need more? Name the parameter: `SymbolicVm::<64>::new()`. The keccak bound
/// is separate and global — see [`crate::keccak::MAX_HASHES`].
pub type SymbolicVM = SymbolicVm<16>;

/// A symbolic Stylus host.
///
/// `SLOTS` bounds distinct storage slots touched in one proof. Exceeding it
/// prunes the path rather than wrapping, so a proof cannot silently check less
/// than it claims — but it can become vacuous, so assert reachability with
/// `kani::cover` if a proof passes implausibly fast.
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
pub struct SymbolicVm<const SLOTS: usize> {
    state: alloc::rc::Rc<RefCell<VmState<SLOTS>>>,
    ctx: Context,
}

struct VmState<const SLOTS: usize> {
    slots: SlotStore<SLOTS>,
}

// Derived Clone would demand `SLOTS: Clone`; write it out instead.
impl<const SLOTS: usize> Clone for SymbolicVm<SLOTS> {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            ctx: self.ctx,
        }
    }
}

impl<const SLOTS: usize> SymbolicVm<SLOTS> {
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
        Self {
            state: alloc::rc::Rc::new(RefCell::new(VmState {
                slots: SlotStore::new(),
            })),
            ctx,
        }
    }

    /// Start from a concrete context and override selected fields.
    ///
    /// ```ignore
    /// let vm = SymbolicVM::concrete_ctx().with_sender(owner);
    /// ```
    pub fn with_sender(mut self, sender: Address) -> Self {
        self.ctx.msg_sender = sender;
        self
    }

    pub fn with_value(mut self, value: U256) -> Self {
        self.ctx.msg_value = value;
        self
    }

    /// A second handle on the *same storage*, at a later clock.
    ///
    /// Unlike the builders above this borrows rather than consuming, because
    /// both handles stay live: the `Rc` is shared, so writes made through the
    /// old one are visible through the new one. That is what lets a proof
    /// advance the block timestamp between calls — `Context` is a plain `Copy`
    /// field, so `From<&H>` hands each contract its own copy and a later
    /// mutation would not be seen.
    ///
    /// ```ignore
    /// let mut v1 = Contract::from(&vm);
    /// v1.claim();                       // at vm's timestamp
    /// let vm2 = vm.with_timestamp(t2);
    /// let mut v2 = Contract::from(&vm2);
    /// v2.claim();                       // sees v1's writes, at t2
    /// ```
    pub fn with_timestamp(&self, block_timestamp: u64) -> Self {
        let mut ctx = self.ctx;
        ctx.block_timestamp = block_timestamp;
        Self { state: self.state.clone(), ctx }
    }

    pub fn context(&self) -> Context {
        self.ctx
    }

    /// Distinct storage slots written so far.
    ///
    /// Use it to confirm a proof isn't silently bounded:
    /// `assert!(vm.slots_touched() < 16)`.
    pub fn slots_touched(&self) -> usize {
        self.state.borrow().slots.touched()
    }

    /// Distinct keccak256 preimages hashed so far in this proof.
    pub fn hashes_taken(&self) -> usize {
        keccak::hashes_taken()
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
        StorageSnapshot(self.state.borrow().slots.clone())
    }

    /// Number of storage slots whose value differs from `before`.
    pub fn slots_changed_since(&self, before: &StorageSnapshot<SLOTS>) -> usize {
        self.state.borrow().slots.changed_since(&before.0)
    }
}

/// Storage frozen at a point in time. Produced by [`SymbolicVm::snapshot`].
pub struct StorageSnapshot<const SLOTS: usize>(SlotStore<SLOTS>);

impl<const SLOTS: usize> Default for SymbolicVm<SLOTS> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const SLOTS: usize> Host for SymbolicVm<SLOTS> {}

// --- the parts a proof actually exercises ----------------------------------

impl<const SLOTS: usize> StorageAccess for SymbolicVm<SLOTS> {
    fn storage_load_bytes32(&self, key: U256) -> B256 {
        self.state.borrow().slots.load(key)
    }
    unsafe fn storage_cache_bytes32(&self, key: U256, value: B256) {
        self.state.borrow_mut().slots.store(key, value);
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
        self.ctx.msg_sender
    }
    fn msg_value(&self) -> U256 {
        self.ctx.msg_value
    }
    fn msg_reentrant(&self) -> bool {
        false
    }
    fn tx_origin(&self) -> Address {
        self.ctx.tx_origin
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
        self.ctx.block_number
    }
    fn block_timestamp(&self) -> u64 {
        self.ctx.block_timestamp
    }
    fn block_gas_limit(&self) -> u64 {
        30_000_000
    }
}

impl<const SLOTS: usize> ChainAccess for SymbolicVm<SLOTS> {
    fn chain_id(&self) -> u64 {
        self.ctx.chain_id
    }
}

impl<const SLOTS: usize> AccountAccess for SymbolicVm<SLOTS> {
    fn balance(&self, _account: Address) -> U256 {
        U256::ZERO
    }
    fn contract_address(&self) -> Address {
        self.ctx.contract_address
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
