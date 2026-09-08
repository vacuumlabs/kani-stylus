# kani-stylus-core

A symbolic ArbOS host, so you can formally verify Arbitrum Stylus contracts with
[Kani](https://model-checking.github.io/kani/).

Instead of writing one test per input, you write one *proof* and Kani checks it
against every input at once — and hands you a concrete counterexample when it
finds one.

```rust
#[kani::proof]
fn adding_never_shrinks_the_counter() {
    let vm = SymbolicVM::concrete_ctx();
    let mut c = Counter::from(&vm);

    let a = any_u256();
    let b = any_u256();
    c.set_number(a);
    c.add_number(b);

    assert!(c.number() >= a);
}
```

That proof **fails** on the stock `cargo stylus new` template, with a witness.

## Setup

```toml
[dependencies]
# `stylus-test` is what makes the generic `From<&H>` impl exist and makes `VM`
# hold a `Box<dyn Host>`. It does not mean you use `TestVM`.
stylus-sdk = { version = "0.10.9", features = ["stylus-test"] }

[dev-dependencies]
kani-stylus-core = { path = "../crates/kani-stylus-core" }
```

Put proofs behind `#[cfg(kani)]` so they don't affect normal builds:

```rust
#[cfg(kani)]
mod proofs {
    use kani_stylus_core::{any_u256, SymbolicVM};
    use super::*;
    // #[kani::proof] fns here
}
```

Then:

```bash
cargo kani --output-format terse                 # all harnesses
cargo kani --harness proofs::my_property         # just one
cargo kani -Z stubbing                           # required if you use mappings
cargo kani -Z concrete-playback --concrete-playback=print --harness <h>
```

## API

| Item | Purpose |
| --- | --- |
| `SymbolicVM::new()` | symbolic storage *and* symbolic sender/value/block |
| `SymbolicVM::concrete_ctx()` | symbolic storage, concrete context — ~2× cheaper |
| `.with_sender(a)` / `.with_value(v)` | pin one context field |
| `any_u256()` / `any_address()` | symbolic values of the right shape |
| `SymbolicVm::<64>` | raise the storage-slot bound from the default 16 |
| `vm.slots_touched()` / `vm.hashes_taken()` | check a bound isn't silently binding |

## Mappings need the keccak stub

Stylus mappings hash through `stylus_sdk::crypto::keccak`, which calls
`alloy_primitives::keccak256` **directly** rather than through the `Host` trait.
Implementing `native_keccak256` is therefore not enough on its own — real keccak
would still reach the solver, which it cannot survive.

Any harness touching a mapping needs:

```rust
#[kani::proof]
#[kani::stub(stylus_sdk::crypto::keccak, kani_stylus_core::keccak_stub)]
fn balances_do_not_alias() { /* ... */ }
```

and `cargo kani -Z stubbing`.

The stub models keccak256 as an **uninterpreted injective function**: the same
preimage always gives the same digest, distinct preimages always give distinct
digests, and nothing else is assumed. The solver never sees a round of keccak.

## Two traps worth knowing before you trust a result

**`U256` arithmetic wraps silently.** `alloy`/`ruint` define `+` and `-` as
`wrapping_add`/`wrapping_sub`, and Kani's automatic overflow checks only cover
*primitive* integers — not library types built on wrapping `u64` limbs. **A
passing proof does not rule out overflow.** Say it explicitly:

```rust
kani::assume(a.checked_add(b).is_some());  // rule it out as a precondition
assert!(result >= a);                      // or assert what you actually want
```

**Bounds prune, so proofs can go vacuous.** Exceeding the storage-slot or hash
bound kills the execution path via `kani::assume(false)` rather than wrapping.
That's safe — a proof can never check *less* than it claims — but an over-tight
bound can leave nothing to check at all. If a proof passes implausibly fast, add
a `kani::cover` for a state you expect to reach, or assert
`vm.slots_touched() < SLOTS`.

## Keeping proofs fast

Solver cost is driven by how wide your symbolic values are, not by how much
contract code runs. Measured on the counter example (Kani 0.67.0, solver time
only, excluding the shared compile):

| Harness | Symbolic inputs | Time |
| --- | --- | --- |
| `starts_at_zero` | none | 10s |
| `set_then_get_roundtrips` | one `U256` | 26s |
| `roundtrip_holds_for_any_transaction_context` | one `U256` + full context | 32s |
| `add_number_is_exactly_wrapping` | two `U256` | 43s |
| `increment_wraps_at_max` | one `U256`, boundary hunt | 44s |
| `add_number_can_decrease_the_counter` | two `U256` | 47s |
| `add_number_is_exact_when_it_does_not_overflow` | two `U256` + `checked_add` precondition | 57s |

Tens of seconds, not minutes — and note a full symbolic transaction context adds
only ~6s over a concrete one. The first `cargo kani` in a session pays a few
minutes to compile the dependency tree; after that it is cached.

In rough order of what to reach for when something *is* slow:

1. **Use the narrowest type the property allows.** If a bug reproduces with
   `u64`-sized balances, prove it there first; widen once it's green.
   Full-width `U256` is right when the property is *about* the boundary — the
   overflow proofs in the examples have to be full width.
2. **Prefer `concrete_ctx()`** unless the property depends on sender, value or
   block.
3. **Constrain hard with `kani::assume`.** Every precondition you state is
   input space the solver doesn't explore.
4. Keep `SLOTS` and the keccak bound just large enough.
5. Iterate with `--harness <name>`; only run the full suite when you mean it.

## What is and isn't modelled

Modelled: persistent storage (bounded), keccak256 (as above), and
`msg` / `block` / `chain` context.

Not modelled — and a proof that reaches one of these **fails loudly** rather
than inventing an answer: cross-contract calls, `CREATE`/`CREATE2`, gas
accounting, and dispatch through the ABI router (proofs call contract methods
directly, so calldata is empty).

Verification also runs natively, against the Rust source. It says nothing about
the compiled WASM or about ArbOS itself.

## Why not the SDK's `TestVM`?

Because it cannot be verified. `TestVM`'s state holds nine `std::HashMap`s;
`HashMap::new()` seeds SipHash from OS randomness through a `getrandom` syscall,
and Kani cannot model foreign functions — verification aborts before reaching
any contract code.

Measured: constructing a `TestVM` fails in 2.4s; binding a contract to one times
out at 420s. The same proof against `SymbolicVM` takes 6s. See
[`kb/50-feasibility.md`](../../kb/50-feasibility.md).

## Examples

[`examples/proofs`](../../examples/proofs) is the place to start — a verified
counter (including the real overflow bug) and a vault with owner-gated methods
and mappings.
