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
| `.with_arbitrary_storage()` | unwritten storage reads as arbitrary values — start from every state at once |
| `kani_stylus_core::proof! { fn ... }` | harnesses that touch mappings, with the storage model's stubs attached |
| `SymbolicVm::<32>` | raise the bound on mapping entries and other large slots from the default 16 |
| `vm.slots_touched()` / `vm.entries_derived()` / `vm.hashes_taken()` | check a bound isn't silently binding |
| `vm.snapshot()` / `vm.slots_changed_since(&s)` | frame conditions — "this call changed nothing else" |

One VM per proof: storage and context are global, which is what makes the host
cheap. Clone a VM, or use `with_timestamp`/`with_sender`, to get more handles;
changing the context changes it for all of them — the next transaction.

## Mappings: declare harnesses with `proof!`

Stylus puts `map[key]` at `keccak256(key ‖ map's slot)`, and computes that
through `stylus_sdk::crypto::keccak` — **directly**, not through the `Host`
trait — so a harness that touches a mapping needs stubs. `proof!` attaches
them:

```rust
kani_stylus_core::proof! {
    fn balances_do_not_alias() { /* ... */ }

    #[kani::should_panic]          // other attributes go through
    fn credit_can_silently_wrap() { /* ... */ }
}
```

and `cargo kani -Z stubbing`. It picks one of two storage models for every
harness in the run:

- **Structured (default).** The SDK's slot derivation for `Address`, `bool`
  and unsigned-integer keys is replaced by an injective numbering of
  `(map, key)` pairs, so nothing is hashed at all. Several times cheaper than
  hashing, and the gap grows with the number of keys. Exact, given that
  distinct mapping entries never share a slot — the collision freedom every
  Solidity and Stylus layout already rests on — so counterexamples are real.
  See [`slots`](src/slots.rs).
- **Precise** — `--features proofs,kani-stylus-core/precise-storage`. The SDK
  derives every slot itself and keccak256 is modelled as an **uninterpreted
  injective function**: the same preimage always gives the same digest,
  distinct preimages distinct digests, and nothing else. The check on the
  abstraction, and the model for `U256`, `B256`, signed-integer and byte-string
  keys in either mode, which Kani cannot stub. Budget for it: combined with
  `with_arbitrary_storage()`, the vault's heaviest lemmas did not finish in 15
  minutes where structured slots take two or three.

`kani_stylus_core::precise_proof!` pins one harness to the precise model.
Written by hand, a harness needs at least
`#[kani::stub(stylus_sdk::crypto::keccak, kani_stylus_core::keccak_stub)]`.
Why this is safe, what each model assumes, and how Certora, hevm and Halmos do
the same: [`kb/36-storage-model.md`](../../kb/36-storage-model.md).

## Nonlinear arithmetic: `arith` and `arith_oracle`

A symbolic `U256` division does not verify as ruint implements it, and even
modelled exactly, facts like "`x * y / z` grows with `y`" are beyond a SAT
solver at 256 bits. Two stub families, for two jobs:

- **`arith`** models division exactly. Counterexamples are real, so use it to
  demonstrate bugs and in every `#[kani::should_panic]` harness.
- **`arith_oracle`** replaces `*` and `/` with uninterpreted functions
  constrained by lemmas (monotonicity, `x * y / y == x`, …), each checked
  against real ruint by `cargo test`. Proofs become cheap; a failure may be
  spurious, so replay it on real arithmetic.

```rust
#[kani::proof]
#[kani::stub(ruint::Uint::wrapping_mul, kani_stylus_core::arith_oracle::mul_stub)]
#[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::arith_oracle::div_stub)]
fn vested_is_monotone_in_time() { /* ... */ }
```

Why, what each assumes, and how Certora and hevm do the same:
[`kb/35-arithmetic-oracle.md`](../../kb/35-arithmetic-oracle.md).

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

Start from an arbitrary state rather than seeding one by hand:

```rust
let vm = SymbolicVM::concrete_ctx().with_sender(from).with_arbitrary_storage();
let mut v = Vault::from(&vm);
let before_total = v.total();       // any value at all, and the same on every read
```

That includes states no sequence of calls reaches, so state any invariant the
lemma needs with `kani::assume`.

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
- **Cover the interesting case.** A `kani::cover!` such as
  `vm.slots_changed_since(&before) == 2` shows the lemma is not passing
  vacuously — that the method can succeed and move what it should.

## Two traps worth knowing before you trust a result

**`U256` arithmetic wraps silently.** `alloy`/`ruint` define `+` and `-` as
`wrapping_add`/`wrapping_sub`, and Kani's automatic overflow checks only cover
*primitive* integers — not library types built on wrapping `u64` limbs. **A
passing proof does not rule out overflow.** Say it explicitly:

```rust
kani::assume(a.checked_add(b).is_some());  // rule it out as a precondition
assert!(result >= a);                      // or assert what you actually want
```

**Some bounds prune, so proofs can go vacuous.** Exceeding `SLOTS` or
`slots::MAX_ENTRIES` fails the proof loudly, but exceeding the keccak oracle's
`MAX_HASHES` kills the execution path via `kani::assume(false)`. That's safe —
a proof can never check *less* than it claims — but an over-tight bound can
leave nothing to check at all. If a proof passes implausibly fast, add a
`kani::cover` for a state you expect to reach.

## Keeping proofs fast

Solver cost is driven by how wide your symbolic values are and how much
storage the proof touches, not by how much contract code runs. Measured on
[`examples/vault`](../../examples/vault) on 2026-09-30 (Kani 0.67.0,
`--no-assertion-reach-checks`, one cgroup per harness):

| Harness | Structured | Precise | Before the storage rework |
| --- | --- | --- | --- |
| `vault::only_owner_can_transfer_ownership` | 7s | 6s | 53s |
| `vault::credit_then_read_roundtrips` | 6s | 24s | 56s |
| `vault::distinct_accounts_do_not_alias` | 22s | 91s | 226s |
| `vault::transfer_does_not_move_any_other_balance` | 41s | 119s | 371s |
| `vault::transfer_conserves_total` | 150s | > 15 min | > 15 min |
| `vault::transfer_from_moves_nothing_else` | 167s | > 15 min | > 15 min |

Structured storage is the default; the other columns and what changed are in
[`kb/36-storage-model.md`](../../kb/36-storage-model.md). The heaviest vault
harnesses now peak at about 2.4 GiB. Memory used to be the binding
constraint — one `cbmc` on a mapping proof needed several GiB — so still run
heavy proofs one at a time, one cgroup each: two at once once OOM-killed a
23 GiB machine and took the editor with it, because terminal children share
its systemd scope. The `systemd-run --user` recipe is in
[`kb/40-toolchain.md`](../../kb/40-toolchain.md).

**Most of the default verification time is not your proof.** With Kani's
assertion-reachability checks on, which is the default, a small harness spent
about three quarters of its time turning the checks' traces into results:
10.5s against 49s without them, at half the memory. Use
`--no-assertion-reach-checks` while iterating and cover non-vacuity with
explicit `kani::cover!`s; run the defaults before you trust a result.

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
   transaction context is cheap on scalar proofs, so `new()` is the right
   default there — but on 2026-09-08 it added 15–35% to mapping proofs, where
   it is worth dropping if the property doesn't depend on the caller.
3. **Constrain hard with `kani::assume`.** Every precondition you state is input
   space the solver doesn't explore.
4. **Split a lemma that asserts several arithmetic identities.** Storage is
   cheap now, so what is left is the `U256` arithmetic you assert. The vault's
   `transfer_from` lemmas ran past 15 minutes as one harness and take two to
   three minutes each as two.
5. Iterate with `--harness <name>`; only run the full suite when you mean it.

## What is and isn't modelled

Modelled: persistent storage (bounded), mapping slot derivation and keccak256
(as above), and `msg` / `block` / `chain` context.

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

All are real, deployable `cargo stylus new` projects with proofs added in
place, next to their existing unit tests — not bespoke verification crates.

**[`examples/counter`](../../examples/counter)** is the place to start: the
stock template. Storage round-trips, three silently-wrapping arithmetic methods,
and a `#[payable]` method proved over every possible `msg_value`.

**[`examples/vault`](../../examples/vault)** covers what a counter cannot:
owner-gated methods over a symbolic caller, mappings (so it needs
`-Z stubbing`), and ERC-20 allowances — a nested map — proved from an
arbitrary state.

**[`examples/vesting`](../../examples/vesting)** adds time and integer
division, and uses [`arith_oracle`](src/arith_oracle.rs).

In each, `cargo test` and `cargo build` behave exactly as they did before —
confirmed by inspecting the built wasm for `kani` and `stylus-test` symbols
(there are none).
