//! A bounded symbolic model of the EVM storage trie.
//!
//! Stylus shares the EVM's storage: a `U256` slot key maps to a `B256` value,
//! and unset slots read as zero. This models it as a fixed-size association
//! list with a linear scan.
//!
//! An array is deliberate. The obvious choice, `std::collections::HashMap`, is
//! **unusable under Kani**: `HashMap::new()` builds a `RandomState`, which seeds
//! SipHash from OS randomness via a `getrandom` syscall, and Kani cannot model
//! foreign functions — verification aborts outright. This is exactly why the
//! SDK's own `TestVM` cannot be verified. See `kb/50-feasibility.md`.
//!
//! At these sizes a linear scan is also simply cheaper for the solver than any
//! hashing scheme would be.

use alloy_primitives::{B256, U256};

/// Bounded slot store. `SLOTS` caps how many *distinct* slots a single proof
/// may touch; repeated access to the same slot is free.
#[derive(Clone)]
pub struct SlotStore<const SLOTS: usize> {
    keys: [U256; SLOTS],
    vals: [B256; SLOTS],
    len: usize,
}

impl<const SLOTS: usize> SlotStore<SLOTS> {
    pub fn new() -> Self {
        Self {
            keys: [U256::ZERO; SLOTS],
            vals: [B256::ZERO; SLOTS],
            len: 0,
        }
    }

    /// Slots written so far. Compare against `SLOTS` in a proof to check the
    /// bound isn't silently binding.
    pub fn touched(&self) -> usize {
        self.len
    }

    pub fn load(&self, key: U256) -> B256 {
        let mut i = 0;
        while i < self.len {
            if self.keys[i] == key {
                return self.vals[i];
            }
            i += 1;
        }
        // Unwritten slots read as zero, matching the EVM.
        B256::ZERO
    }

    pub fn store(&mut self, key: U256, value: B256) {
        let mut i = 0;
        while i < self.len {
            if self.keys[i] == key {
                self.vals[i] = value;
                return;
            }
            i += 1;
        }
        // Prune paths that would exceed the bound rather than overwrite one.
        // A proof that needs more slots should raise `SLOTS`, not silently
        // verify a smaller contract than the one written.
        kani::assume(self.len < SLOTS);
        self.keys[self.len] = key;
        self.vals[self.len] = value;
        self.len += 1;
    }
}

impl<const SLOTS: usize> Default for SlotStore<SLOTS> {
    fn default() -> Self {
        Self::new()
    }
}
