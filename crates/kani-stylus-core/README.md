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
| `vm.snapshot()` / `vm.slots_changed_since(&s)` | frame conditions — "this call changed nothing else" |

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

## Conservation properties: prove them by local deltas

The property everyone wants from a token is `total == sum of every balance`.
**A bounded model checker cannot state it** — there is no quantifying over 2^160
addresses, and summing N of them explicitly did not converge even at N = 2.

Decompose it instead. For each method, prove *locally*, from an **arbitrary**
pre-state, that

1. it moves `total` by exactly the net amount it moves balances by, and
2. it changes nothing else — the **frame condition**.

Global conservation then follows by induction over any call sequence: if the sum
invariant held before a call, (1) and (2) say it holds after.

**Be explicit that the induction step is a hand argument.** Kani proves the
per-method lemmas; composing them over sequences is on paper. This is the
standard decomposition — it is what Certora rules do for Solidity, and nobody
sums 2^160 balances — but say so rather than implying the sum was checked.

Two ways to write the frame condition, both in
[`examples/vault`](../../examples/vault):

```rust
// Cheap: count slots. Needs no knowledge of slot derivation.
let before = vm.snapshot();
v.transfer(to, amount).unwrap();
assert!(vm.slots_changed_since(&before) <= 2, "transfer wrote a third slot");

// Stronger: a symbolic third party. `unsat` then covers *every* other
// address at once — the universal quantifier you get for free from proving
// no counterexample exists. Costs one more mapping access.
let other = any_address();
kani::assume(other != from && other != to);
let before_other = v.balance_of(other);
v.transfer(to, amount).unwrap();
assert_eq!(v.balance_of(other), before_other);
```

Note `<=`, not `==`: a frame condition is an upper bound on what moved. With
`amount == 0` nothing changes, and that is fine.

Two practical notes:

- **Use `checked_add`/`checked_sub` in the assertions too**, not just in the
  contract. Bare `+` on `U256` wraps, so `assert_eq!(after, before + amount)`
  quietly proves something weaker than you meant.
- **Tighten `SLOTS`.** `changed_since` costs up to `SLOTS^2` symbolic 256-bit
  comparisons, and the conservation lemmas simply cannot be *encoded* within
  5 GiB above `SymbolicVm::<4>`. This buys memory, not time — see
  [`kb/50-feasibility.md`](../../kb/50-feasibility.md). Pair it with
  `kani::cover!(vm.slots_touched() == 3)` so a too-tight bound fails loudly
  instead of passing vacuously.

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
| `vault::credit_then_read_roundtrips` | 1 account, 1 write | 142s |
| `vault::credit_can_silently_wrap` | 1 account, 2 writes | 257s |
| `vault::credit_checked_never_wraps` | 1 account, 2 guarded writes | 270s |
| `vault::credit_checked_moves_total_by_the_same_delta` | 1 account, delta + frame | 236s |
| `vault::transfer_does_not_move_any_other_balance` | 3 accounts, symbolic frame | 359s |
| `vault::distinct_accounts_do_not_alias` | 2 accounts, 2 guarded writes | 478s |
| `vault::transfer_conserves_total` | 2 accounts, delta + frame | 495s |

Under ten minutes per harness, and the full 17-harness suite across both
examples runs in **46 minutes** (was ~125). Most of that came from storing the
keccak oracle's memo table as 256-bit words rather than byte arrays — the win
scales with the number of distinct hashes, up to -81% on the three-account
lemma. Four other candidates (`SLOTS`, `MAX_HASHES`, digest width, solver
choice) were each worth <=10%; see
[`kb/50-feasibility.md`](../../kb/50-feasibility.md).

The **binding constraint is memory, not time**: one `cbmc` on a mapping proof
needs several GiB, so these proofs are effectively serial on a laptop
regardless of core count. Run them one at a time, one cgroup each — two at once
OOM-killed a 23 GiB machine and took the editor with it, because terminal
children share its systemd scope. The `systemd-run --user` recipe is in
[`kb/40-toolchain.md`](../../kb/40-toolchain.md).

Two measurement notes: `--harness` is a **substring** filter, so use `--exact`
with the fully qualified name for a single harness; and never compare timings
across runs with different background load — the same config measured 200s idle
and 259s under contention.

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
4. **Keep `SLOTS` just large enough — for memory, not speed.** Measured
   2026-09-09: varying `SLOTS` from 2 to 16 changes the formula 2.5x but leaves
   solve time flat (157s / 196s / 185s / 192s). What it buys is *encodability*:
   `transfer_conserves_total` cannot be encoded within 5 GiB above
   `SymbolicVm::<4>`. Since memory is the binding constraint, a tight bound is
   still the difference between a proof running and not — it just will not make
   a running proof faster. Pair it with `kani::cover!` so a too-tight bound
   fails loudly instead of passing vacuously.
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
