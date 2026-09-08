# Project: kani-stylus

**Goal.** Let Rust developers formally verify Arbitrum Stylus smart contracts
with [Kani](https://model-checking.github.io/kani/), AWS's SMT-backed bounded
model checker, instead of only unit-testing them.

The full pitch (hackathon framing, grant trajectory, MVP scope) is in
[`../proposal.md`](../proposal.md). Read [50-feasibility.md](50-feasibility.md)
alongside it — the proposal's central technical premise needs revising.

## What "done" looks like for the MVP

1. A crate that gives a Stylus contract a **symbolic** host environment, so
   `kani::any()` values flow through storage reads, calldata, and `msg::sender()`.
2. At least two real proofs over a non-trivial contract (ERC-20 balance
   conservation; owner-only access control).
3. A demonstration that Kani produces a **concrete counterexample** when a bug
   is deliberately injected — this is the part that makes the demo land.
4. One command that runs the whole proof suite.

## Repo layout

```
kani-stylus/
├── README.md            getting started (links out to upstream docs)
├── proposal.md          the hackathon/grant pitch
├── kb/                  this knowledge base
└── stylus-samples/
    └── counter/         working Stylus contract, built with `cargo stylus new`
```

### Where new code should go

Not yet created — decide before writing the first crate. The shape the proposal
implies, adapted to what the SDK actually offers (see [50-feasibility.md](50-feasibility.md)):

```
kani-stylus/
├── Cargo.toml                    [workspace] over the crates below
├── crates/
│   ├── kani-stylus-core/         SymbolicVM: an impl of stylus_core::Host
│   │                             backed by kani::any() + a bounded slot map
│   └── kani-stylus/              proof macros / harness ergonomics
│                                 (fold into -core if it stays thin)
└── stylus-samples/
    ├── counter/                  existing; smallest possible proof target
    └── erc20/                    the MVP proof target
```

Making the root a Cargo workspace means `cargo kani` at the root can sweep every
harness in one run, which is deliverable #4 above.

### On vendoring the Kani repo

[`context.md`](../context.md) asked whether to clone
[model-checking/kani](https://github.com/model-checking/kani) locally.
**Recommendation: don't commit it.** The book at
<https://model-checking.github.io/kani/> covers the user-facing surface, and the
installed toolchain already ships the library sources under `~/.kani/kani-0.67.0/`.
If someone needs to read compiler internals, a shallow clone into a gitignored
path is enough:

```bash
git clone --depth 1 https://github.com/model-checking/kani vendor/kani  # vendor/ is gitignored
```

## Status

- [x] Stylus counter contract builds and has passing unit tests
- [x] Kani 0.67.0 and cargo-stylus 0.10.9 installed locally
- [ ] Feasibility spike: does `cargo kani` get through a `stylus-test` build at all?
      (see [50-feasibility.md](50-feasibility.md) — this gates everything else)
- [ ] `kani-stylus-core` symbolic host
- [ ] ERC-20 proof harnesses
- [ ] Injected-defect counterexample demo
