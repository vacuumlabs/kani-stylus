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

Add this to the project `cargo stylus new` gave you — no restructuring needed:

```toml
[dependencies]
kani-stylus-core = { path = "path/to/kani-stylus-core", optional = true }

[dev-dependencies]
# For `cargo test`, per the Stylus testing guide.
stylus-sdk = { version = "0.10.9", features = ["stylus-test"] }

[features]
# Verification only. Off by default.
proofs = ["dep:kani-stylus-core", "stylus-sdk/stylus-test"]
```

Two things about that shape are load-bearing, and both are easy to get wrong:

- **`stylus-test` must never be on in a real build.** It swaps the ArbOS host
  calls for a mockable one, and with it enabled every hostio becomes a
  `panic!` — a contract built that way is broken on chain. Keeping it behind an
  opt-in feature leaves `cargo build` and `cargo stylus check` untouched.
- **`kani-stylus-core` must be a regular optional dependency, not a
  dev-dependency.** `cargo kani` builds the *lib* target, where
  dev-dependencies aren't available. (`stylus-sdk`'s `stylus-test` gets pulled
  in for verification by the `proofs` feature above, and separately by the
  dev-dependency for `cargo test`.)

Put proofs behind `#[cfg(kani)]` so they don't affect normal builds:

```rust
#[cfg(kani)]
mod proofs {
    use kani_stylus_core::{any_u256, SymbolicVM};
    use super::*;
    // #[kani::proof] fns here
}
```

A guard makes the feature impossible to forget:

```rust
#[cfg(all(kani, not(feature = "proofs")))]
compile_error!("proofs need the `proofs` feature: cargo kani --features proofs");
```

Then:

```bash
cargo kani --features proofs --output-format terse        # all harnesses
cargo kani --features proofs --harness proofs::my_property
cargo kani --features proofs -Z stubbing                  # if you use mappings
cargo kani --features proofs -Z concrete-playback \
    --concrete-playback=print --harness <h>
```

Your ordinary workflow is unchanged: `cargo test`,
`cargo build --target wasm32-unknown-unknown --release` and `cargo stylus check`
all behave exactly as before, because none of them enable `proofs`.

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

Solver cost is driven by how wide your symbolic values are and how many storage
slots you touch, not by how much contract code runs. Measured on the examples
(Kani 0.67.0, solver time only; the dependency compile is shared and cached).

**Scalar storage — seconds.**

| Harness | Time |
| --- | --- |
| `counter::starts_at_zero` | 12s |
| `counter::set_then_get_roundtrips` | 39s |
| `counter::add_number_is_exact_when_it_does_not_overflow` | 65s |
| `vault::only_owner_can_transfer_ownership` | 72s |
| `counter::mul_number_can_wrap` | 80s |

**Mappings — minutes, growing with the number of accesses.**

| Harness | Mapping work | Time |
| --- | --- | --- |
| `vault::credit_then_read_roundtrips` | 1 account, 1 write | 238s |
| `vault::credit_can_silently_wrap` | 1 account, 2 writes | 561s |
| `vault::credit_checked_never_wraps` | 1 account, 2 guarded writes | 675s |
| `vault::distinct_accounts_do_not_alias` | 2 accounts, 2 guarded writes | 1236s |

A property spanning several mapping accounts is the current limit: a two-account
`total == balance(a) + balance(b)` conservation proof has not been seen to
converge. See [`kb/50-feasibility.md`](../../kb/50-feasibility.md).

In rough order of what to reach for when something is slow:

1. **Use the narrowest type the property allows.** If a bug reproduces with
   `u64`-sized balances, prove it there first; widen once it's green. Full-width
   `U256` is right when the property is *about* the boundary — the overflow
   proofs in the examples have to be full width.
2. **Reach for `concrete_ctx()` on mapping-heavy proofs.** A fully symbolic
   transaction context costs only ~6s on scalar proofs, so `new()` is the right
   default there — but it adds 15–35% to mapping proofs, where it is worth
   dropping if the property doesn't depend on the caller.
3. **Constrain hard with `kani::assume`.** Every precondition you state is input
   space the solver doesn't explore.
4. Keep `SLOTS` just large enough; each extra slot costs a symbolic 256-bit
   comparison on every load and store.
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

Both are real, deployable `cargo stylus new` projects with proofs added in
place, next to their existing unit tests — not bespoke verification crates.

**[`examples/counter`](../../examples/counter)** is the place to start: the
stock template. Storage round-trips, three silently-wrapping arithmetic methods,
and a `#[payable]` method proved over every possible `msg_value`.

**[`examples/vault`](../../examples/vault)** covers what a counter cannot:
owner-gated methods over a symbolic caller, and mappings (so it needs
`-Z stubbing`).

In both, `cargo test` and `cargo build` behave exactly as they did before —
confirmed by inspecting the built wasm for `kani` and `stylus-test` symbols
(there are none).
