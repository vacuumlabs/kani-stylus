# Your first proof

This walkthrough takes a fresh `cargo stylus new` project to a formal proof, a
real bug with its counterexample, and a fix that is proved correct for every
input. Allow about 20 minutes, about ten of them waiting for compiles.

You need Rust, `cargo-stylus` and Kani installed. See the
[README](../README.md#1-install-the-toolchains). Every command and output below
was run on 2026-10-02 with cargo-stylus 0.10.9, stylus-sdk 0.10.9 and Kani
0.67.0. [`examples/counter`](../examples/counter/) is the same template with a
fuller set of proofs.

## 1. Start from the template

```bash
cargo stylus new first-proof
cd first-proof
```

`src/lib.rs` is the stock `Counter`: one `uint256`, and methods to set, add to,
multiply and increment it. It comes with one unit test, `test_counter`.

As generated, that test does not build. `cargo test` stops with
``could not find `testing` in `stylus_sdk` ``, because the template doesn't
enable the SDK's `stylus-test` feature. The next step fixes this too.

## 2. Add kani-stylus-core

All the setup is in `Cargo.toml`. Add the lines marked `+`:

```diff
 [dependencies]
 alloy-primitives = "1.5.7"
 alloy-sol-types = "1.5.7"
 stylus-sdk = "0.10.9"
+kani-stylus-core = { git = "https://github.com/vacuumlabs/kani-stylus", optional = true }
+
+[dev-dependencies]
+stylus-sdk = { version = "0.10.9", features = ["stylus-test"] }

 [features]
 ...
 contract-client-gen = []
+proofs = ["dep:kani-stylus-core", "stylus-sdk/stylus-test"]
```

and at the end of the file:

```toml
[lints.rust]
unexpected_cfgs = { level = "warn", check-cfg = ['cfg(kani)'] }
```

What each part does:

- **`kani-stylus-core`** is the symbolic ArbOS host. It is optional, so only
  the `proofs` feature pulls it in.
- **`proofs`** also turns on the SDK's `stylus-test` feature, which lets a
  contract run against a host you supply. Never enable it in a build you
  deploy: it turns every real host call into a `panic!`. Behind an opt-in
  feature, it can't reach `cargo build` or `cargo stylus check`.
- **The dev-dependency** is what makes the template's own unit test build.
- **The lint entry** stops `cargo build` warning about `#[cfg(kani)]`, which
  only `cargo kani` sets.

Check that the unit test passes now:

```bash
cargo test
```

```text
test test::test_counter ... ok
```

## 3. Prove something for every input

Add this at the end of `src/lib.rs`:

```rust
#[cfg(all(kani, not(feature = "proofs")))]
compile_error!("proofs need the `proofs` feature: cargo kani --features proofs");

#[cfg(kani)]
mod proofs {
    use super::*;
    use kani_stylus_core::{any_u256, SymbolicVM};

    #[kani::proof]
    fn set_then_get_roundtrips() {
        let vm = SymbolicVM::new();
        let mut contract = Counter::from(&vm);

        let n = any_u256();
        contract.set_number(n);

        assert_eq!(contract.number(), n);
    }
}
```

It reads like the unit test, with two differences:

- **`SymbolicVM::new()`** takes the place of `TestVM`. The caller, the value
  sent and the block are symbolic, so Kani considers every possibility. Storage
  starts empty, as in a fresh deployment.
- **`any_u256()`** is not a random number. It stands for every `U256` at once,
  so this one harness covers all 2²⁵⁶ values of `n`.

`#[cfg(kani)]` keeps the module out of every normal build. The
`compile_error!` line is there for when you forget `--features proofs`.

Run it:

```bash
cargo kani --features proofs --output-format terse
```

The first run compiles the SDK's dependency tree through Kani's compiler. That
took 2.5 minutes on our laptop, and later runs reuse it. Trimmed, the output
looks like this:

```text
warning: Found the following unsupported constructs:
             - caller_location (1)
             - foreign function (2)

         Verification will fail if one or more of these constructs is reachable.
...
VERIFICATION RESULT:
 ** 0 of 1106 failed (67 unreachable)
VERIFICATION:- SUCCESSFUL
Verification Time: 8.221629s
Manual Harness Summary:
Complete - 1 successfully verified harnesses, 0 failures, 1 total.
```

`SUCCESSFUL` means Kani found no value of `n` that breaks the assertion. That
is a proof over the whole input space, not a sample of it.

The rest of the output is routine:

- **The warning** lists code in the dependencies that Kani can't model. A
  proof that reached it would fail, so it can't produce a wrong answer. No
  proof in this walkthrough or the examples reaches it, and you'll see the
  warning on every run.
- **1106 checks** are your assertion plus the ones Kani adds itself, such as
  bounds and primitive-integer overflow.
- **The 67 unreachable** checks are in code this harness never runs.

## 4. Find a bug

Add a second harness inside `mod proofs`. Adding to a counter should never make
it smaller:

```rust
    #[kani::proof]
    fn add_number_never_shrinks_the_counter() {
        let vm = SymbolicVM::new();
        let mut contract = Counter::from(&vm);

        let a = any_u256();
        let b = any_u256();
        contract.set_number(a);
        contract.add_number(b);

        assert!(contract.number() >= a, "add_number shrank the counter");
    }
```

```bash
cargo kani --features proofs --output-format terse \
    --harness add_number_never_shrinks_the_counter
```

```text
VERIFICATION RESULT:
 ** 1 of 1158 failed (67 unreachable)
Failed Checks: "add_number shrank the counter"
 File: "src/lib.rs", line 136, in proofs::add_number_never_shrinks_the_counter

VERIFICATION:- FAILED
Verification Time: 15.128468s
```

Kani found inputs that make the assertion false.

The template's `add_number` does `new_number + self.number.get()`, and `U256`
addition wraps around: `alloy` defines `+` as `wrapping_add`. Where Solidity
would revert, this contract silently stores the wrapped sum, and its unit test
passes.

Kani's automatic overflow checks only cover primitive integers like `u64`, so
they never flag a `U256`. This bug surfaced only because the harness asserted
the property. **A passing proof says nothing about `U256` overflow unless you
asserted something that overflow would break.**

## 5. Replay the counterexample

Ask Kani to write the failing inputs into your source as a unit test:

```bash
cargo kani --features proofs --output-format terse \
    -Z concrete-playback --concrete-playback=inplace \
    --harness add_number_never_shrinks_the_counter
```

On our laptop this run took 94 s, against 15 s without playback. It ends with:

```text
INFO: Now modifying the source code to include the concrete playback unit test:
  - kani_concrete_playback_add_number_never_shrinks_the_counter_17891704372126321095.
```

The new `#[test]` sits right after the harness. Its values are raw bytes:
first the symbolic transaction context, then `a` and `b`. To see them as
numbers, print them from the harness, just before the `assert!`:

```rust
        eprintln!("a      = {a:#x}\nb      = {b:#x}\nstored = {:#x}", contract.number());
```

Then replay the test. It runs the harness on those exact values as ordinary
Rust, which is why the print works:

```bash
cargo kani playback -Z concrete-playback --features proofs -- kani_concrete_playback
```

```text
---- proofs::kani_concrete_playback_add_number_never_shrinks_the_counter_17891704372126321095 stdout ----
a      = 0xffffffffffffffff800000000000000180000000000000018000000000000001
b      = 0xfffffffffffffffe7ffffffffffffffe7ffffffffffffffe7fffffffffffffff
stored = 0xfffffffffffffffe000000000000000000000000000000000000000000000000

thread '…' panicked at src/lib.rs:137:9:
add_number shrank the counter
```

The two inputs add up to more than 2²⁵⁶, so the stored sum wraps around to
less than `a`. The first replay builds the test profile, which took 2m40s.
Your values may differ from these, but they always break the assertion.

## 6. Fix the contract, and prove the fix

Make `add_number` revert instead of wrapping. In `src/lib.rs`, replace it with:

```rust
    /// Increments `number` by `new_number`, or reverts if the sum overflows.
    pub fn add_number(&mut self, new_number: U256) -> Result<(), Vec<u8>> {
        let sum = self
            .number
            .get()
            .checked_add(new_number)
            .ok_or_else(|| b"overflow".to_vec())?;
        self.number.set(sum);
        Ok(())
    }
```

`add_number` now returns a `Result`, so in `test_counter` the call becomes
`contract.add_number(U256::from(3)).unwrap();`.

Now state what the fixed method promises. It succeeds exactly when the sum
fits, and then it stores the exact sum. Replace the failing harness with:

```rust
    #[kani::proof]
    fn add_number_is_exact_or_reverts() {
        let vm = SymbolicVM::new();
        let mut contract = Counter::from(&vm);

        let a = any_u256();
        let b = any_u256();
        contract.set_number(a);
        let ok = contract.add_number(b).is_ok();

        assert_eq!(ok, a.checked_add(b).is_some(), "reverts exactly on overflow");
        if ok {
            assert_eq!(contract.number(), a + b, "stores the exact sum");
        }
        kani::cover!(ok, "add_number can succeed");
    }
```

Delete the playback test from step 5 as well. It calls the old harness.

```bash
cargo kani --features proofs --output-format terse
```

```text
VERIFICATION RESULT:
 ** 0 of 1129 failed (66 unreachable)
 ** 1 of 1 cover properties satisfied
VERIFICATION:- SUCCESSFUL
Verification Time: 16.765108s
...
Manual Harness Summary:
Complete - 2 successfully verified harnesses, 0 failures, 2 total.
```

For every pair of inputs, `add_number` now reverts exactly when the sum would
overflow, and otherwise stores the sum exactly.

The `kani::cover!` line is the habit to keep. An assertion inside a branch that
can never run passes vacuously. `cover!` asks Kani to show that the branch *can*
run. **Read its line yourself:** if the branch can't run, Kani prints
`0 of 1 cover properties satisfied` and still reports `VERIFICATION:-
SUCCESSFUL`. We checked this with a cover that can never hold.

If you can't change the contract, you can still prove what holds. State the
precondition with `kani::assume(a.checked_add(b).is_some())`, and everything
after it is proved for the inputs that satisfy it. See
`add_number_is_exact_when_it_does_not_overflow` in
[`examples/counter`](../examples/counter/src/lib.rs).

To practise, try the rest of the template. `mul_number`, `increment` and
`add_from_msg_value` all wrap the same way. `examples/counter` finds the bug in
the first two; `increment` breaks at exactly one value, `U256::MAX`. For
`add_from_msg_value` it proves the method exact for every value sent that
doesn't overflow, using `SymbolicVM::new().with_value(sent)`.

## 7. Your normal workflow is unchanged

```bash
cargo test
cargo build --target wasm32-unknown-unknown --release
```

`test_counter` passes, and the contract builds to an 18.8 KB
`first_proof.wasm`. Neither command enables `proofs`, so the wasm contains no
proof code. A search of it for `kani`, `SymbolicVM` and `stylus_test` finds
nothing.

`cargo stylus check` is unaffected for the same reason. It needs a Stylus RPC to
finish; see the [Stylus quickstart](https://docs.arbitrum.io/stylus/quickstart).

## Next: your own contract

The same steps apply to any Stylus contract: add the `Cargo.toml` lines, put
harnesses in a `#[cfg(kani)]` module, and run `cargo kani --features proofs`.
What changes is what you reach for:

| If the property involves… | Use | Worked example |
| --- | --- | --- |
| who is calling | `SymbolicVM::new()` for every caller, `.with_sender(addr)` to pin one, `kani::assume(caller != owner)` | `only_owner_can_transfer_ownership` in [`examples/vault`](../examples/vault/src/lib.rs) |
| mappings | harnesses declared inside `kani_stylus_core::proof! { … }`, run with `-Z stubbing` | [`examples/vault`](../examples/vault/src/lib.rs) |
| every starting state, not just a fresh contract | `SymbolicVM::new().with_arbitrary_storage()`, with `kani::assume` for any invariant you rely on | `transfer_conserves_total` in [`examples/vault`](../examples/vault/src/lib.rs) |
| "this call changes nothing else" | `vm.snapshot()` and `vm.slots_changed_since(&before)`, or a symbolic third party | `transfer_does_not_move_any_other_balance` in [`examples/vault`](../examples/vault/src/lib.rs) |
| time, or `x * y / z` | `.with_timestamp(t)`, and the `arith` / `arith_oracle` division stubs | [`examples/vesting`](../examples/vesting/src/lib.rs) |

The full API is in the [crate README](../crates/kani-stylus-core/README.md).

Before you trust a result:

- **Overflow has to be asserted.** As in step 4, nothing flags `U256`
  wrapping for you.
- **Check for vacuity.** If a proof passes suspiciously fast, add a
  `kani::cover!` for a state you expect it to reach, and check that the summary
  counts it as satisfied. An unsatisfied cover does not fail the run.
- **Keep `TestVM`, and anything else holding a `std::HashMap`, out of proofs.**
  `HashMap` seeds itself randomly, which makes Kani abort before it reaches your
  contract.
- **Run heavy proofs one at a time.** A proof over several mapping entries can
  take a few GiB, and parallel ones can exhaust a laptop.
  [`kb/40-toolchain.md`](../kb/40-toolchain.md) has a capped recipe.
- **`--harness` matches substrings.** Use `--exact --harness proofs::name` to
  run exactly one.
- **Cross-contract calls aren't modelled.** A proof that reaches one fails
  loudly, rather than passing.
