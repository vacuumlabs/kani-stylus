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

1. **Does `cargo kani` complete a build against `stylus-sdk --features
   stylus-test`?** — *Spike run 2026-09-08; see "Smoke test" below.* The SDK
   pulls in `alloy-primitives`, `ruint`, `keccak`, `rclite`; any of them could
   contain something Kani rejects.
2. **Does dynamic dispatch through `Box<dyn Host>` blow up the encoding?**
   Every host call goes through a trait object under `stylus-test`. If proofs
   don't converge, monomorphising past the box is the first lever.
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
  frees roughly the first 24 hours. Spend the freed time on solver convergence,
  not on scope.

## Smoke test (2026-09-08)

A minimal crate depending on `stylus-sdk 0.10.9 --features stylus-test`, with a
`sol_storage!` counter and one `#[kani::proof]` harness asserting
`set_number(n); number() == n` for symbolic `n`.

Kept at
`$SCRATCH/smoke/` during the spike; fold it into `stylus-samples/` once it's
known-good.

**Result: _pending — first run in progress; the crate compiled through the Kani
compiler and reached `goto-instrument`, so the SDK builds under Kani._** Record
the outcome here, and if it passes, promote the harness into the repo as the
first regression test.
