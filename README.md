# kani-stylus

Bounded model checking for [Arbitrum Stylus](https://docs.arbitrum.io/stylus/gentle-introduction)
smart contracts, using [Kani](https://model-checking.github.io/kani/).

Stylus lets you write smart contracts in Rust. Kani is a bounded model checker
for Rust that proves properties over *all* inputs within a bound — and hands you
a concrete counterexample when one fails. This repo is about wiring the two
together: give a Stylus contract a symbolic host environment, then prove things
like "transfers conserve total supply" and "only the owner can call this"
instead of testing them one input at a time.

**Status: working.** [`crates/kani-stylus-core`](crates/kani-stylus-core/) gives
your contract a symbolic ArbOS host; [`examples/proofs`](examples/proofs/) shows
it verifying a counter and a vault, including mappings and owner-gated methods.
It finds real bugs — the stock `cargo stylus new` template has a silent `U256`
overflow, and Kani produces the exact witness.

[`proposal.md`](proposal.md) is the original pitch; parts of it are superseded
by what the code turned out to need — see [`kb/50-feasibility.md`](kb/50-feasibility.md).

## Getting started

### 1. Install the toolchains

Rust plus the wasm target, then the two CLIs:

```bash
rustup target add wasm32-unknown-unknown
cargo install --force cargo-stylus
cargo install --locked kani-verifier && cargo kani setup
```

- Stylus prerequisites: <https://docs.arbitrum.io/stylus/fundamentals/prerequisites>
- Kani installation: <https://model-checking.github.io/kani/install-guide.html>

Verify with `cargo stylus --version` and `cargo kani --version`.
Versions this repo has been exercised against are in [`kb/40-toolchain.md`](kb/40-toolchain.md).

### 2. Run the sample contract

[`stylus-samples/counter/`](stylus-samples/counter/) is a working Stylus contract
(the `cargo stylus new` template) with `TestVM`-based unit tests.

```bash
cd stylus-samples/counter
cargo test                                            # unit tests
cargo build --target wasm32-unknown-unknown --release # build the wasm
cargo stylus check                                    # would it activate on-chain?
```

### 3. Run the proofs

```bash
./verify.sh                                        # the whole suite
./verify.sh set_then_get_roundtrips                # one harness
./verify.sh --playback add_number_can_decrease_the_counter
```

The first run compiles the dependency tree through the Kani compiler and takes a
few minutes; after that individual harnesses are seconds.

The one to look at first is
`counter::proofs::add_number_can_decrease_the_counter` in
[`examples/proofs/src/counter.rs`](examples/proofs/src/counter.rs). It proves
that the stock Stylus counter template can be made to *shrink* by adding to it,
because `alloy`'s `U256 + U256` is `wrapping_add` and never panics. `--playback`
turns that into a runnable test with the exact values.

Writing your own: see the [crate README](crates/kani-stylus-core/README.md).
The shape is

```rust
#[cfg(kani)]
mod proofs {
    use kani_stylus_core::{any_u256, SymbolicVM};
    use super::*;

    #[kani::proof]
    fn my_invariant() {
        let vm = SymbolicVM::concrete_ctx();
        let mut c = MyContract::from(&vm);
        let x = any_u256();
        kani::assume(/* precondition */ true);
        // ... call methods, then assert the property
    }
}
```

### 4. Learn the two halves

**Stylus** — start with the [gentle introduction](https://docs.arbitrum.io/stylus/gentle-introduction)
and the [quickstart](https://docs.arbitrum.io/stylus/quickstart). Then the parts
that matter here: [storage](https://docs.arbitrum.io/stylus/fundamentals/data-types/storage),
[testing with `TestVM`](https://docs.arbitrum.io/stylus/fundamentals/testing-contracts),
and the [SDK overview](https://docs.arbitrum.io/stylus/reference/overview).
Arbitrum publishes an LLM-friendly index of all of it at
<https://docs.arbitrum.io/llms.txt>.

**Kani** — the [tutorial](https://model-checking.github.io/kani/kani-tutorial.html)
is short and worth doing end to end. Then the
[attribute reference](https://model-checking.github.io/kani/reference/attributes.html)
(`#[kani::proof]`, `#[kani::unwind]`, `#[kani::stub]`) and, before you trust any
result, [limitations](https://model-checking.github.io/kani/limitations.html)
and [what Kani doesn't catch](https://model-checking.github.io/kani/undefined-behaviour.html).

A fuller, categorised link list is in [`kb/10-links.md`](kb/10-links.md).

### 5. Read the knowledge base

[`kb/`](kb/) holds the project's shared context — architecture notes taken from
reading the SDK source, Kani's limits, and the open feasibility questions.
It links out to upstream docs rather than duplicating them.

Two findings matter most, if you read nothing else:

- The host interface is already a safe Rust trait (`stylus_core::Host`), not raw
  FFI — but the SDK's own mock host, `TestVM`, **cannot be verified by Kani**,
  because `std::HashMap` seeds SipHash through a `getrandom` syscall. A
  purpose-built symbolic host is mandatory, and it is fast.
- **`U256` arithmetic silently wraps.** `alloy`/`ruint` define `+` as
  `wrapping_add`, and Kani's automatic overflow checks don't cover library
  types. Overflow must be asserted explicitly.

See [`kb/20-stylus.md`](kb/20-stylus.md) and [`kb/50-feasibility.md`](kb/50-feasibility.md).

## Layout

```
verify.sh          run the proof suite
proposal.md        the hackathon / grant pitch
kb/                knowledge base for humans and agents
crates/
  kani-stylus-core/  the library: SymbolicVM, slot store, keccak oracle
examples/
  proofs/          worked examples — counter and vault, fully verified
spikes/
  kani-smoke/      the original feasibility probe, kept for the record
stylus-samples/
  counter/         deployable Stylus contract from `cargo stylus new`
```

## License

Not yet chosen. Apache-2.0 OR MIT would match both `stylus-sdk` and `kani`.
