# kani-stylus

Bounded model checking for [Arbitrum Stylus](https://docs.arbitrum.io/stylus/gentle-introduction)
smart contracts, using [Kani](https://model-checking.github.io/kani/).

Stylus lets you write smart contracts in Rust. Kani is a bounded model checker
for Rust that proves properties over *all* inputs within a bound — and hands you
a concrete counterexample when one fails. This repo is about wiring the two
together: give a Stylus contract a symbolic host environment, then prove things
like "transfers conserve total supply" and "only the owner can call this"
instead of testing them one input at a time.

Status: early. See [`proposal.md`](proposal.md) for the pitch and
[`kb/50-feasibility.md`](kb/50-feasibility.md) for what's actually been verified.

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

### 3. Learn the two halves

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

### 4. Read the knowledge base

[`kb/`](kb/) holds the project's shared context — architecture notes taken from
reading the SDK source, Kani's limits, and the open feasibility questions.
It links out to upstream docs rather than duplicating them.

Most important single finding, if you read nothing else: as of stylus-sdk 0.10.9
the host interface is already a safe Rust trait (`stylus_core::Host`), not raw
FFI, which changes how this project should be built. See
[`kb/20-stylus.md`](kb/20-stylus.md) and [`kb/50-feasibility.md`](kb/50-feasibility.md).

## Layout

```
proposal.md        the hackathon / grant pitch
kb/                knowledge base for humans and agents
stylus-samples/
  counter/         working Stylus contract; the smallest proof target
```

## License

Not yet chosen. Apache-2.0 OR MIT would match both `stylus-sdk` and `kani`.
