//! A minimal symbolic `Host` for Kani — the prototype of what `kani-stylus-core`
//! will become.
//!
//! The point is to avoid `TestVM` entirely. `TestVM`'s `VMState` holds nine
//! `std::HashMap`s, which drag SipHash and hashbrown's SSE2 group probing into
//! the goto program; see `kb/50-feasibility.md`. Here storage is a fixed-size
//! array with a linear scan, which is trivial for CBMC to encode.
//!
//! This works because `stylus-proc` generates
//! `impl<H: Host + Clone + 'static> From<&H> for Counter`, so any `Host` can be
//! injected. The `stylus-test` *feature* is still required (it is what makes
//! that impl exist and makes `VM` hold a `Box<dyn Host>`), but the `stylus-test`
//! *crate*'s code stays unreachable.

use alloc::vec::Vec;
use core::cell::RefCell;

use stylus_sdk::alloy_primitives::{Address, B256, U256};
use stylus_sdk::stylus_core::*;

/// Storage slots tracked per proof. Keep tiny — this bounds the model.
pub const MAX_SLOTS: usize = 4;

#[derive(Clone)]
struct SlotStore {
    keys: [U256; MAX_SLOTS],
    vals: [B256; MAX_SLOTS],
    len: usize,
}

impl SlotStore {
    fn new() -> Self {
        Self {
            keys: [U256::ZERO; MAX_SLOTS],
            vals: [B256::ZERO; MAX_SLOTS],
            len: 0,
        }
    }

    fn load(&self, key: U256) -> B256 {
        let mut i = 0;
        while i < self.len {
            if self.keys[i] == key {
                return self.vals[i];
            }
            i += 1;
        }
        // Unwritten slots read as zero, matching EVM semantics.
        B256::ZERO
    }

    fn store(&mut self, key: U256, value: B256) {
        let mut i = 0;
        while i < self.len {
            if self.keys[i] == key {
                self.vals[i] = value;
                return;
            }
            i += 1;
        }
        // Prune paths that would exceed the bound rather than silently wrapping.
        kani::assume(self.len < MAX_SLOTS);
        self.keys[self.len] = key;
        self.vals[self.len] = value;
        self.len += 1;
    }
}

/// Symbolic execution context. Drawn once at construction so that repeated
/// calls to e.g. `msg_sender()` agree with each other within a proof, the way
/// they would within a real transaction.
#[derive(Clone, Copy)]
struct Ctx {
    msg_sender: Address,
    msg_value: U256,
    contract_address: Address,
    block_number: u64,
    block_timestamp: u64,
    chain_id: u64,
}

impl Ctx {
    fn symbolic() -> Self {
        Self {
            msg_sender: Address::from(kani::any::<[u8; 20]>()),
            msg_value: U256::from_be_bytes(kani::any::<[u8; 32]>()),
            contract_address: Address::from(kani::any::<[u8; 20]>()),
            block_number: kani::any(),
            block_timestamp: kani::any(),
            chain_id: kani::any(),
        }
    }

    /// Cheaper variant: concrete context, symbolic storage only. Use when the
    /// property under test doesn't depend on transaction context.
    fn concrete() -> Self {
        Self {
            msg_sender: Address::ZERO,
            msg_value: U256::ZERO,
            contract_address: Address::ZERO,
            block_number: 0,
            block_timestamp: 0,
            chain_id: 42161,
        }
    }
}

#[derive(Clone)]
pub struct SymbolicVM {
    slots: alloc::rc::Rc<RefCell<SlotStore>>,
    ctx: Ctx,
}

impl SymbolicVM {
    /// Fully symbolic caller, value, block context.
    pub fn new() -> Self {
        Self {
            slots: alloc::rc::Rc::new(RefCell::new(SlotStore::new())),
            ctx: Ctx::symbolic(),
        }
    }

    /// Symbolic storage, concrete transaction context — cheaper to solve.
    pub fn with_concrete_ctx() -> Self {
        Self {
            slots: alloc::rc::Rc::new(RefCell::new(SlotStore::new())),
            ctx: Ctx::concrete(),
        }
    }

    pub fn set_sender(&mut self, sender: Address) {
        self.ctx.msg_sender = sender;
    }
}

impl Default for SymbolicVM {
    fn default() -> Self {
        Self::new()
    }
}

impl Host for SymbolicVM {}

// --- the parts that matter -------------------------------------------------

impl StorageAccess for SymbolicVM {
    fn storage_load_bytes32(&self, key: U256) -> B256 {
        self.slots.borrow().load(key)
    }
    unsafe fn storage_cache_bytes32(&self, key: U256, value: B256) {
        self.slots.borrow_mut().store(key, value);
    }
    fn flush_cache(&self, _clear: bool) {}
}

impl MessageAccess for SymbolicVM {
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
        self.ctx.msg_sender
    }
}

impl CalldataAccess for SymbolicVM {
    // Proofs call contract methods directly rather than through the ABI router,
    // so calldata is not modelled yet. See open question #7 in the KB.
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

impl CryptographyAccess for SymbolicVM {
    // TODO: model as an uninterpreted injective function so that storage
    // mappings (ERC-20 balances) work without a bit-level keccak. Real keccak
    // is intractable for the solver. Not needed for plain-field contracts.
    fn native_keccak256(&self, _input: &[u8]) -> B256 {
        B256::from(kani::any::<[u8; 32]>())
    }
}

// --- context, cheap concrete answers ---------------------------------------

impl BlockAccess for SymbolicVM {
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

impl ChainAccess for SymbolicVM {
    fn chain_id(&self) -> u64 {
        self.ctx.chain_id
    }
}

impl AccountAccess for SymbolicVM {
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

impl MeteringAccess for SymbolicVM {
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

impl MemoryAccess for SymbolicVM {
    fn pay_for_memory_grow(&self, _pages: u16) {}
}

impl RawLogAccess for SymbolicVM {
    fn emit_log(&self, _input: &[u8], _num_topics: usize) {}
    fn raw_log(&self, _topics: &[B256], _data: &[u8]) -> Result<(), &'static str> {
        Ok(())
    }
}

// --- out of scope: reaching these is a proof failure, by design -------------

unsafe impl UnsafeDeploymentAccess for SymbolicVM {
    unsafe fn create1(
        &self,
        _code: *const u8,
        _code_len: usize,
        _endowment: *const u8,
        _contract: *mut u8,
        _revert_data_len: *mut usize,
    ) {
        unimplemented!("contract deployment is not modelled")
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
        unimplemented!("contract deployment is not modelled")
    }
}

unsafe impl UnsafeCallAccess for SymbolicVM {
    unsafe fn call_contract(
        &self,
        _to: *const u8,
        _data: *const u8,
        _data_len: usize,
        _value: *const u8,
        _gas: u64,
        _outs_len: &mut usize,
    ) -> u8 {
        unimplemented!("cross-contract calls are not modelled")
    }
    unsafe fn static_call_contract(
        &self,
        _to: *const u8,
        _data: *const u8,
        _data_len: usize,
        _gas: u64,
        _outs_len: &mut usize,
    ) -> u8 {
        unimplemented!("cross-contract calls are not modelled")
    }
    unsafe fn delegate_call_contract(
        &self,
        _to: *const u8,
        _data: *const u8,
        _data_len: usize,
        _gas: u64,
        _outs_len: &mut usize,
    ) -> u8 {
        unimplemented!("cross-contract calls are not modelled")
    }
}
