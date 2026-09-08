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
mod proofs {
    use super::*;
    use stylus_sdk::testing::*;

    #[kani::proof]
    #[kani::unwind(4)]
    fn set_then_get_roundtrips() {
        let vm = TestVM::default();
        let mut c = Counter::from(&vm);
        let n: u64 = kani::any();
        c.set_number(U256::from(n));
        assert_eq!(c.number(), U256::from(n));
    }
}
