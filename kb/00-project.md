# Project: kani-stylus

**Goal.** Let Rust developers formally verify Arbitrum Stylus smart contracts
with [Kani](https://model-checking.github.io/kani/), AWS's SMT-backed bounded
model checker, instead of only unit-testing them.

The pitch (hackathon framing, what it proves, limits, roadmap) is in
[`../proposal.md`](../proposal.md), rewritten 2026-10-02 to match this KB. The
original 2026-09-08 draft assumed an older SDK; what it got wrong is recorded
in [50-feasibility.md](50-feasibility.md).

## What "done" looks like for the MVP

1. A crate that gives a Stylus contract a **symbolic** host environment, so
   `kani::any()` values flow through storage reads, calldata, and `msg::sender()`.
2. At least two real proofs over a non-trivial contract (ERC-20 balance
   conservation; owner-only access control). **Both done** — conservation by
   local-delta lemmas rather than by summing balances, which a bounded model
   checker cannot express. See [50-feasibility.md](50-feasibility.md).
3. A demonstration that Kani produces a **concrete counterexample** when a bug
   is deliberately injected — this is the part that makes the demo land.
4. One command that runs the whole proof suite.

## Repo layout

```
kani-stylus/
├── Cargo.toml           workspace over crates/ only
├── verify.sh            run the proof suites
├── README.md            getting started (links out to upstream docs)
├── docs/first-proof.md  walkthrough: fresh project → first proof → counterexample → fix
├── proposal.md          the hackathon/grant pitch
├── kb/                  this knowledge base
├── crates/
│   └── kani-stylus-core/  the library: SymbolicVM, storage model, keccak and arithmetic oracles
└── examples/            each a real, deployable `cargo stylus new` project
    ├── counter/         storage, arithmetic, payable methods
    ├── vault/           access control, mappings, ERC-20 allowances
    └── vesting/         time and integer division
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
- [x] **Worked examples**, each a real `cargo stylus new` project with proofs
      added in place: [`examples/counter`](../examples/counter) (7 harnesses),
      [`examples/vault`](../examples/vault) (14) and
      [`examples/vesting`](../examples/vesting) (13 by default, plus 7 behind
      `slow-proofs`). Latest runs, without reach checks: `./verify.sh counter`
      7 of 7 and `./verify.sh vault` 14 of 14 in 645s (2026-10-01); vesting's
      13 each end as they should, run one at a time (2026-09-30). See
      [36-storage-model.md](36-storage-model.md#measured). The last
      `./verify.sh` over every project was 2026-09-10 (17 of 17, 46 minutes,
      before vesting and the allowance lemmas).
- [x] **One command runs the whole suite** — [`verify.sh`](../verify.sh), with
      `--playback` for counterexamples.
- [x] **Conservation proved (2026-09-09).** `examples/vault` gained `transfer`
      and three local-delta lemmas (463s / 1836s / 1904s). The summed form
      `total == Σ balance(aᵢ)` is unstatable in a bounded model checker and was
      abandoned; conservation is decomposed into per-method delta + frame
      lemmas, with the induction over call sequences a **disclosed hand
      argument**. New API: `vm.snapshot()` / `vm.slots_changed_since()`.
- [x] **Storage modelled at the level contracts use it (2026-09-30).** A
      zero-sized host, a two-tier store, and structured mapping slots by
      default, with a `precise-storage` feature for the SDK's own keccak
      derivation; plus `with_arbitrary_storage()` for arbitrary pre-state.
      See [36-storage-model.md](36-storage-model.md).
- [x] **"Write your first proof" walkthrough (2026-10-02):**
      [`docs/first-proof.md`](../docs/first-proof.md). It goes from a fresh
      `cargo stylus new` project to a passing proof, a failing one, its replayed
      counterexample and a proved fix. Every command and output in it was run on
      a fresh template with a `git` dependency on this repo. No newcomer has
      followed it yet, which is the roadmap's "done when" test.
- [ ] A more substantial verification target. ERC-20 is a plausible stepping
      stone but is **not** settled — the interesting goal is something closer to
      a real DeFi contract. No longer blocked on mapping proof cost since the
      2026-09-30 storage model; what is left in heavy harnesses is the `U256`
      arithmetic they assert.

**What comes after this list, and in what order, is in
[60-roadmap.md](60-roadmap.md).**

### Deliberately not doing

- **No upstream contributions.** An SDK feature split (`mock-host`, giving the
  `Box<dyn Host>` VM without pulling in `stylus-test`'s RPC stack) would be a
  clean improvement and is documented in
  [50-feasibility.md](50-feasibility.md) — but it stays a documented option, not
  a task. Everything works without it; the cost is compile time only.
