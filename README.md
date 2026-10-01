# kani-stylus

<img src="assets/logo.svg" alt="" width="120" align="right">

Bounded model checking for [Arbitrum Stylus](https://docs.arbitrum.io/stylus/gentle-introduction)
smart contracts, using [Kani](https://model-checking.github.io/kani/).

Stylus lets you write smart contracts in Rust. Kani is a bounded model checker
for Rust that proves properties over *all* inputs within a bound — and hands you
a concrete counterexample when one fails. This repo is about wiring the two
together: give a Stylus contract a symbolic host environment, then prove things
like "transfers conserve total supply" and "only the owner can call this"
instead of testing them one input at a time.

[`crates/kani-stylus-core`](crates/kani-stylus-core/) gives
your contract a symbolic ArbOS host, and it drops into an ordinary
`cargo stylus new` project — see [`examples/counter`](examples/counter/), 
with proofs added alongside its unit tests and `cargo test`,
`cargo build` and `cargo stylus check` all unaffected. It finds real bugs: that
stock template has a silent `U256` overflow, and Kani produces the exact
witness.

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

[`examples/counter/`](examples/counter/) is a working Stylus contract
(the `cargo stylus new` template) with `TestVM`-based unit tests.

```bash
cd examples/counter
cargo test                                            # unit tests
cargo build --target wasm32-unknown-unknown --release # build the wasm
cargo stylus check                                    # would it activate on-chain? (you need to have your nitro devnode running for this, see https://docs.arbitrum.io/stylus/quickstart for instructions)
```

### 3. Run the proofs

```bash
./verify.sh                                        # every project
./verify.sh counter                                # just the real contract
./verify.sh -h set_then_get_roundtrips             # one harness
./verify.sh --playback credit_can_silently_wrap    # print a counterexample
```

The first run compiles the dependency tree through the Kani compiler and takes a
few minutes; individual harnesses are seconds after that.

#### It goes in your normal Stylus project

[`examples/counter`](examples/counter/) is an ordinary
`cargo stylus new` contract. Proofs live in `src/lib.rs` next to the
`#[cfg(test)]` module, and the whole setup is three lines of `Cargo.toml`:

```toml
[dependencies]
kani-stylus-core = { path = "...", optional = true }

[features]
proofs = ["dep:kani-stylus-core", "stylus-sdk/stylus-test"]
```

Everything stays opt-in, so the ordinary workflow is untouched — verified on
that project:

| Command | Result |
| --- | --- |
| `cargo test` | passes (the template's own `test_counter`) |
| `cargo build --target wasm32-unknown-unknown --release` | 18.5 KB cdylib, no kani or `stylus-test` symbols |
| `cargo stylus check` | compiles and sizes the contract at 6.0 KB (its activation step needs a Stylus RPC) |
| `cargo kani --features proofs` | 7 harnesses |

`stylus-test` **must** stay behind that feature: it replaces the real ArbOS host
calls with a mockable one, so a contract built with it enabled would `panic!` on
every hostio. The `proofs` feature keeps it out of every non-verification build.

Where a unit test pins one input, a proof covers the whole space — and on this
contract that finds a real bug. `add_number_can_decrease_the_counter` shows the
stock template can be made to *shrink* by adding to it, because `alloy`'s
`U256 + U256` is `wrapping_add` and never panics. `--playback` prints the exact
values.

[`examples/vault`](examples/vault/) covers what a counter can't: mappings and
owner-gated access control.

[`examples/vesting`](examples/vesting/) adds time and integer division. Work in
progress.

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
verify.sh          run the proof suites
proposal.md        the hackathon / grant pitch
kb/                knowledge base for humans and agents
crates/
  kani-stylus-core/  the library: SymbolicVM, slot store, keccak oracle
examples/          each one a real, deployable `cargo stylus new` project
  counter/         storage, arithmetic, payable methods
  vault/           access control and mappings
  vesting/         time and integer division (WIP)
```

## License

Not yet chosen. Apache-2.0 OR MIT would match both `stylus-sdk` and `kani`.
