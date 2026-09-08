# Feasibility: proposal vs. reality

[`../proposal.md`](../proposal.md) was written as a pitch, not a spec. This file
records where it matches the code and where it doesn't. Update it as experiments
land; it's the live risk register.

Last checked 2026-09-08 against `stylus-sdk` / `stylus-core` **0.10.9** and Kani
**0.67.0**.

## The central premise needs revising — in our favour

> **Proposal:** "Rust smart contracts … cannot currently leverage Rust-native
> formal verification engines like Kani … because the stylus-sdk depends on
> low-level ArbOS foreign function interface (FFI) declarations (`extern "C"`)
> that are provided dynamically by the node runtime." The MVP is therefore
> "stub implementations for the five most critical Stylus ArbOS host
> operations", intercepting `extern "C"` declarations.

**That describes an older SDK.** As of 0.10.9 the SDK already abstracts the host
behind a safe Rust trait — see [20-stylus.md](20-stylus.md) for the full layering:

- `stylus_core::Host` is a composed trait covering every host operation, with
  `storage_load_bytes32`, `read_args`, `msg_sender` etc. as ordinary safe
  methods. Its own doc comment says it "may be implemented by test frameworks as
  an easier way of mocking hostio invocations".
- Under `--features stylus-test`, `stylus_sdk::hostio` emits **no `extern "C"`
  blocks at all** — the `vm_hooks!` macro expands to functions that `panic!`.
- `stylus_sdk::host::VM` holds a `Box<dyn Host>` in that configuration, and
  contracts are built as `Contract::from(&vm)`. Injection is already supported.
- `stylus_sdk::testing::TestVM` is a working concrete implementation of exactly
  this shape, already used in `stylus-samples/counter/src/lib.rs`.

### What this means

The hard part the proposal budgets Hours 00–24 for — FFI interception and
symbol resolution — mostly **does not exist**. The work becomes: implement
`stylus_core::Host` for a `SymbolicVM` whose methods return `kani::any()`.
That is a large, mechanical trait impl, and `TestVM` is the reference to
copy from.

This is good news for delivery and **bad news for the pitch as written**. The
novelty claim has to move. Honest positioning:

- *Not*: "we made Kani able to see past FFI."
- *Instead*: "the SDK's `Host` trait makes mocking possible but only
  concretely — `TestVM` gives you one execution per test. kani-stylus supplies
  a **symbolic** `Host` plus the storage model, harness ergonomics, and property
  library that turn a single harness into a proof over all inputs."

The demo value is unchanged and arguably stronger: counterexamples from an
injected bug are just as compelling, and the story is now "verification is
one crate away for any Stylus contract" rather than a deep FFI hack. Reviewers
who know the SDK will spot the stale premise, so fix it before submission.

**Before relying on any of this, confirm which SDK version the target contracts
use.** The proposal's framing may be accurate for pre-0.8 SDKs, where
`msg::sender()` and friends were free functions hitting `hostio` directly.

## Open questions

Ordered by how much they gate the plan.

1. ~~**Does `cargo kani` complete a build against `stylus-sdk --features
   stylus-test`?**~~ — **Answered 2026-09-08: yes, it builds and instruments;
   but a trivial harness does not converge in 25 minutes.** See
   "Smoke test" below. This is now the project's gating risk, restated as:
   **can the dependency surface be pruned enough for proofs to converge?**
   Two concrete causes identified (runtime regex in `stylus-core`; a 268-crate
   tree including tokio/reqwest under `--features stylus-test`), both with
   plausible fixes.
2. **Does dynamic dispatch through `Box<dyn Host>` blow up the encoding?**
   Every host call goes through a trait object under `stylus-test`. If proofs
   don't converge, monomorphising past the box is the first lever. Not yet
   isolated — the regex noise in the spike swamps any signal about dispatch.
3. **How do we model keccak256?** Needed for storage mappings (ERC-20 balances)
   and unavoidable for the flagship proof. Real keccak is intractable for an SMT
   solver. Plan: uninterpreted injective function. Needs prototyping, and the
   assumption must be disclosed in results.
4. **Do `U256` operations converge?** `ruint`'s `U256` is 4×`u64` limbs. 256-bit
   symbolic arithmetic is expensive. Mitigation: prove over narrower values
   first, widen once green.
5. **Is OpenZeppelin `rust-contracts-stylus` ERC-20 tractable, or do we need our
   own minimal ERC-20?** The OZ target is far more credible for a grant. A
   hand-rolled minimal ERC-20 is the fallback if OZ's abstraction depth defeats
   the solver. Decide early — it shapes Hours 24–36.
6. **`rust-toolchain.toml` interaction.** See [40-toolchain.md](40-toolchain.md).
7. **Does `#[public]`/`sol_storage!` macro-generated code verify cleanly?**
   `stylus-proc` generates routers and storage accessors. Proving at the
   *method* level (calling `contract.transfer(..)` directly) sidesteps the ABI
   router entirely and is the pragmatic MVP path. Proving through raw calldata
   into the router — which is what "panic freedom over arbitrary calldata"
   really requires — is strictly harder. The proposal conflates the two; the
   MVP should do method-level and say so.

## Scope judgements

- The proposal's out-of-scope list (cross-contract calls, full dynamic ABI
  decoding, AST linting) is sensible. Keep it.
- "Verify proof convergence in Kani" is listed as a milestone task but is the
  main technical risk of the whole project. It deserves a timebox and a
  pre-agreed fallback (narrower integer widths, smaller storage bound, minimal
  ERC-20 instead of OZ).
- The 48–72h schedule is plausible *given* the trait-impl finding above, which
  frees roughly the first 24 hours — but the smoke test says that freed time is
  already spoken for by dependency pruning and solver convergence. Treat the
  schedule as fully committed, not as having slack.

## Smoke test (2026-09-08) — ran, did not converge

A minimal crate depending on `stylus-sdk 0.10.9 --features stylus-test`, with a
`sol_storage!` counter and one `#[kani::proof] #[kani::unwind(4)]` harness
asserting `set_number(n); number() == n` for a symbolic `n: u64`. Kept at
[`spikes/kani-smoke/`](../spikes/kani-smoke/), so the result is reproducible.

**Result: compiles and instruments cleanly; killed at a 25-minute timeout during
CBMC symbolic execution, with no verification result.**

That splits open question #1 into a clear yes and a clear no:

- ✅ **The toolchain works.** `cargo kani` built `stylus-sdk` with
  `--features stylus-test` through the Kani compiler, ran `goto-instrument`,
  and entered symbolic execution. Nothing in the SDK is rejected outright, and
  our `#[kani::unwind(4)]` bound was respected ("Not unwinding loop … iteration 4").
- ❌ **A trivial harness does not converge in 25 minutes.** Solver cost, not
  language support, is the binding constraint.

### Why it's slow — two concrete causes, both fixable

**1. `stylus-core` compiles regexes at runtime.**
`stylus-core-0.10.9/src/sol.rs` opens with:

```rust
lazy_static! {
    static ref UINT_REGEX:  Regex = Regex::new(r"^uint(\d+)$").unwrap();
    static ref INT_REGEX:   Regex = Regex::new(r"^int(\d+)$").unwrap();
    static ref BYTES_REGEX: Regex = Regex::new(r"^bytes(\d+)$").unwrap();
}
```

So every Stylus contract transitively builds a regex engine to parse Solidity
type names. Under Kani that engine is *symbolically executed*: the spike log is
dominated by `aho-corasick` and SIMD `memchr`
(`One::<__m128i>::count_raw`, `kani::models::intrinsics::simd_bitmask_impl::<i8, 16>`)
being unwound over and over.

None of this is reachable from contract logic we care about.
**`#[kani::stub]` these out, or stub `stylus_core::sol`'s callers, before
anything else.** This is the single highest-leverage fix and should be the first
thing tried when work resumes.

**2. `--features stylus-test` drags in a JSON-RPC stack.**
The feature is defined as `stylus-test = ["dep:stylus-test", "dep:rclite", ...]`,
and `stylus-test` depends on `alloy-provider`. Measured dependency footprint of
the smoke crate: **268 crates**, including `tokio`, `reqwest`, `hyper`,
`serde_json`. That is an absurd surface to hand a model checker.

The awkward part: the `Box<dyn Host>` form of `stylus_sdk::host::VM` — the
injection seam this whole project depends on — exists *only* under
`cfg(feature = "stylus-test")`. Today you cannot get host injection without also
getting the RPC client.

**This is the real architectural problem, and it is a better story than the one
in the proposal.** Options, best first:

1. **Upstream a feature split** in `stylus-sdk`: a `mock-host` (or `dyn-host`)
   feature that enables the `Box<dyn Host>` VM and `stylus-proc/stylus-test`
   codegen *without* `dep:stylus-test`. Small, obviously-correct PR; makes
   `stylus-sdk` verification-friendly for everyone; and "we contributed the hook
   upstream" is a genuinely strong line in a grant application.
2. Patch locally via `[patch.crates-io]` against a vendored `stylus-sdk` while
   the upstream PR is in flight, so the hackathon isn't blocked on review.
3. Bypass `VM` entirely — have harnesses construct storage types against a
   `SymbolicVM` directly, if `stylus-proc`'s generated code permits it. Needs
   checking against `stylus-proc` source.

### Revised read on the plan

Feasibility is **not** established yet, and the schedule risk has moved. The
proposal budgets Hours 00–24 for FFI stubbing that turns out to be unnecessary;
that time is now clearly needed for dependency pruning and solver convergence
instead. Net timeline is probably unchanged, but the work is different work.

Recommended next spike, in order:
1. Re-run the smoke harness with `regex`/`memchr` paths stubbed. If it converges
   in minutes, the project is on.
2. Prototype the `mock-host` feature split locally and measure the crate count.
3. Only then move to `U256` and keccak-backed mappings (open questions #3, #4).
