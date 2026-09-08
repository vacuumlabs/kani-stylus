//! Transaction and block context for a proof.
//!
//! Values are drawn **once**, when the VM is built, not on each host call. That
//! matters: if `msg_sender()` returned a fresh `kani::any()` every time it were
//! called, a contract that checks `sender == owner` and then acts on that fact
//! would be verified against an attacker who can change identity mid-call. The
//! context has to be fixed for the duration of a transaction, exactly as it is
//! on chain.

use alloy_primitives::{Address, U256};

/// Symbolic execution context, fixed for the lifetime of one proof.
#[derive(Clone, Copy)]
pub struct Context {
    pub msg_sender: Address,
    pub msg_value: U256,
    pub tx_origin: Address,
    pub contract_address: Address,
    pub block_number: u64,
    pub block_timestamp: u64,
    pub chain_id: u64,
}

impl Context {
    /// Everything symbolic: the strongest setting, and the right default when
    /// you don't know which context values a property depends on.
    pub fn symbolic() -> Self {
        let sender = any_address();
        Self {
            msg_sender: sender,
            msg_value: any_u256(),
            tx_origin: sender,
            contract_address: any_address(),
            block_number: kani::any(),
            block_timestamp: kani::any(),
            chain_id: kani::any(),
        }
    }

    /// Everything concrete. Cheaper to solve; use when the property genuinely
    /// doesn't depend on context (pure storage round-trips, arithmetic).
    ///
    /// Measured on the counter example: roughly 14s concrete vs 26s symbolic.
    pub fn concrete() -> Self {
        Self {
            msg_sender: Address::ZERO,
            msg_value: U256::ZERO,
            tx_origin: Address::ZERO,
            contract_address: Address::ZERO,
            block_number: 0,
            block_timestamp: 0,
            chain_id: 42161, // Arbitrum One
        }
    }
}

/// A symbolic 20-byte address.
pub fn any_address() -> Address {
    Address::from(kani::any::<[u8; 20]>())
}

/// A symbolic 256-bit unsigned integer, unconstrained over the full range.
///
/// Note this really is the full range — including values near `U256::MAX` where
/// `alloy`'s wrapping arithmetic bites. That is usually what you want; narrow it
/// with `kani::assume` when the property has a legitimate precondition.
pub fn any_u256() -> U256 {
    U256::from_be_bytes(kani::any::<[u8; 32]>())
}
