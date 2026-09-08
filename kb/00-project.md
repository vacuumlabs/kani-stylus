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
├── Cargo.toml           workspace over crates/ only
├── verify.sh            run the proof suites
├── README.md            getting started (links out to upstream docs)
├── proposal.md          the hackathon/grant pitch
├── kb/                  this knowledge base
├── crates/
│   └── kani-stylus-core/  the library: SymbolicVM, slot store, keccak oracle
├── examples/            each a real, deployable `cargo stylus new` project
│   ├── counter/         storage, arithmetic, payable methods
│   └── vault/           access control and mappings
└── spikes/
    └── kani-smoke/      the original feasibility probe, kept for the record
```

Everything under `examples/` is **excluded** from the workspace, deliberately.
Each one is a standalone, deployable project with its own toolchain pin, exactly
as `cargo stylus new` produces it — that is the point of them: they show
kani-stylus dropping into an ordinary Stylus setup rather than into a bespoke
workspace. (The `rust-toolchain.toml` pin turned out **not** to interfere with
`cargo kani`, so that is not the reason.)

### Where new code should go

The library lives in `crates/kani-stylus-core`. It is a single crate rather than
the `-core` plus macro split the proposal imagined — there turned out to be
nothing for a proc macro to do, since `#[kani::proof]` already exists and
`SymbolicVM` is an ordinary value. Add a macro crate only if a real ergonomic
need shows up.

Worked examples go in `examples/`, one directory per contract, each created
with `cargo stylus new`. They are the main usability deliverable: what a
newcomer reads to learn the tool, and the proof that it works on a normal
project.

### On vendoring the Kani repo

The project's original scoping notes asked whether to clone
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
- [x] **Feasibility resolved (2026-09-08).** `TestVM` cannot be verified by Kani
      at all — `std::HashMap`'s `RandomState` needs a `getrandom` syscall. A
      purpose-built symbolic host works and is fast: symbolic set-then-get goes
      from a 25-minute timeout to **14s**. See [50-feasibility.md](50-feasibility.md).
- [x] **`kani-stylus-core` crate** — `SymbolicVM` implementing the full
      `stylus_core::Host` trait, a bounded slot store, and the keccak oracle.
      Workspace at the repo root.
- [x] **Keccak256 as an uninterpreted injective function**
      ([`keccak.rs`](../crates/kani-stylus-core/src/keccak.rs)). Note it must be
      wired in with `#[kani::stub]`: Stylus mappings call
      `stylus_sdk::crypto::keccak` directly, *not* through the `Host` trait, so
      implementing `native_keccak256` alone is not enough.
- [x] **Worked examples** in [`examples/vault`](../examples/vault) — a
      verified counter (with the real overflow bug and its counterexample) and a
      vault covering access control and mappings.
- [ ] Sharpen the usability story: a short "write your first proof" walkthrough,
      and a single command that runs the whole suite.
- [ ] A more substantial verification target. ERC-20 is a plausible stepping
      stone but is **not** settled — the interesting goal is something closer to
      a real DeFi contract. Decide once the basics are solid.

### Deliberately not doing

- **No upstream contributions.** An SDK feature split (`mock-host`, giving the
  `Box<dyn Host>` VM without pulling in `stylus-test`'s RPC stack) would be a
  clean improvement and is documented in
  [50-feasibility.md](50-feasibility.md) — but it stays a documented option, not
  a task. Everything works without it; the cost is compile time only.
