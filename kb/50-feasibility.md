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

## Feasibility spike (2026-09-08) — resolved: `TestVM` is unusable, `SymbolicVM` works

Spike crate: [`spikes/kani-smoke/`](../spikes/kani-smoke/). A `sol_storage!`
counter plus staged harnesses that add one layer at a time, so the cost cliff
can be located rather than guessed at.

### Result: `TestVM` cannot be verified by Kani at all

| Harness | What it adds | Result |
| --- | --- | --- |
| `h0_empty` | nothing (baseline) | ✅ SUCCESSFUL, 131s — almost entirely compile time |
| `h1_vm_only` | `TestVM::default()` | ❌ **FAILED in 2.4s of solver time**, 8 of 6707 checks failed, 6699 undetermined |
| `h2_contract_only` | `Counter::from(&vm)` | ❌ timeout at 420s |

The `h1` failure is the whole story:

```
Failed Checks: call to foreign "C" function `syscall` is not currently
supported by Kani.
  File: libc-0.2.177/src/unix/linux_like/linux/mod.rs, line 6372,
  in std::sys::random::linux::getrandom::getrandom
```

`TestVM::default()` builds a `VMState`, which holds **nine `std::HashMap`s**
(`storage: HashMap<U256, B256>`, `balances`, `code_storage`, four call-return
maps, …). Every `HashMap::new()` constructs a `RandomState`, which seeds SipHash
from OS randomness — a raw `getrandom` syscall. Kani cannot model foreign
functions, so verification aborts and everything downstream goes undetermined.
Kani additionally reported 13 foreign functions, ~90 atomics and a thread-local
as present-and-unsupported.

**This is a hard incompatibility, not a performance problem.** No amount of
stubbing, unwinding, or solver tuning makes `TestVM` verifiable, because the
obstruction is in `std::HashMap`'s constructor.

### Result: a purpose-built symbolic host works, and is fast

`spikes/kani-smoke/src/symbolic_vm.rs` implements `stylus_core::Host` directly:
storage is a fixed `MAX_SLOTS`-entry array with a linear scan, transaction
context is drawn once at construction, and unmodelled operations
(`create1`, `call_contract`, …) are `unimplemented!()` so that a proof touching
them fails loudly instead of silently.

| Harness | What it proves | Checks | Time |
| --- | --- | --- | --- |
| `s1_contract_only` | contract binds to host | 348 | ✅ 6s |
| `s2_read_only` | one symbolic storage read | 538 | ✅ 8s |
| `s3_starts_at_zero` | fresh contract reads zero | 539 | ✅ 8s |
| `s4_set_then_get` | storage round-trips for all `n: u64` | 1044 | ✅ 14s |
| `s5_add_no_overflow` | `add_number` exact for `u64`-widened operands | 1044 | ✅ 20s |
| `s6_symbolic_ctx` | same, with fully symbolic sender/value/block | 1047 | ✅ 26s |

The direct comparisons are stark:

| | `TestVM` | `SymbolicVM` |
| --- | --- | --- |
| bind contract to host | timeout at 420s | **6s** |
| symbolic set-then-get | timeout at 25 min | **14s** |

Full symbolic transaction context (`s6`) costs 26s against 14s for a concrete
one — cheap enough that symbolic context can be the default.

This works because `stylus-proc` generates

```rust
impl<H: stylus_core::Host + Clone + 'static> From<&H> for Counter
```

— generic over *any* `Host`. `TestVM` is not privileged; it is merely the one
implementation that ships. The `stylus-test` **feature** is still required
(it is what makes that impl exist at all, and what makes `VM` hold a
`Box<dyn Host>`), but the `stylus-test` **crate**'s code never has to be
reachable.

### What this changes

1. **`kani-stylus-core` is not optional.** The proposal framed a symbolic host
   as a nice-to-have over existing mocking. It is the only way to run Kani on a
   Stylus contract at all. That is a *stronger* pitch than the original FFI
   story and it is defensible against a reviewer who knows the SDK — the SDK's
   own mock host provably does not verify.
2. **The 268-crate dependency concern drops in priority.** `tokio`, `reqwest`
   and `hyper` cost compile time but never enter the goto program as long as
   nothing constructs a `TestVM`.

   An upstream `mock-host` feature split — giving the `Box<dyn Host>` VM
   without `dep:stylus-test` — would cut build times and make the intent
   explicit. **This is a documented option only; we are not contributing it
   upstream.** Everything works without it, and the cost is compile time.
3. **Avoid `std` collections everywhere in the verification path.** Any
   `HashMap`/`HashSet` reachable from a harness reintroduces `getrandom`. Use
   fixed arrays, `BTreeMap`, or a `HashMap` with a deterministic hasher.

### Finding: Kani does NOT catch `U256` overflow for free

The proposal lists "arithmetic overflow absence" as something Kani checks
automatically. **For Stylus contracts this is false**, and the reason matters.

`alloy`'s `U256` is `ruint::Uint<256, 4>`, and `ruint/src/add.rs` ends with:

```rust
impl_bin_op!(Add, add, AddAssign, add_assign, wrapping_add);
impl_bin_op!(Sub, sub, SubAssign, sub_assign, wrapping_sub);
```

So `a + b` on `U256` **is** `wrapping_add`. It never panics. Internally it
combines limbs with `carrying_add` on `u64`, which is explicitly wrapping, so
Kani's built-in overflow checks — which only fire on primitive integer
operations — see nothing to complain about either.

Measured: a harness applying `add_number` to two fully symbolic `U256` values
reports `0 of 1043 failed`. There is no panic to find.

Consequences:

- **The stock Stylus counter template silently wraps.** `add_number` and
  `mul_number` in `stylus-samples/counter` have no overflow protection.
  Solidity >= 0.8 would revert here; Rust on Stylus does not.
- Every Stylus contract doing token arithmetic with bare `+`/`-`/`*` on `U256`
  has the same exposure, and neither `cargo test` nor a naive `cargo kani` run
  will surface it.
- **This raises the project's value.** kani-stylus is not just a plumbing layer
  that makes Kani runnable; it has to ship the arithmetic properties Kani cannot
  infer. "Prove your token math doesn't wrap" is a concrete, demonstrable pitch
  with a real counterexample behind it.

Practically, proof obligations should take the shape of `d3` in the spike:
`kani::assume(a.checked_add(b).is_some())` for the intended-behaviour proof,
plus a separate harness showing the unguarded version wraps.

### Result: defect detection works

The other half of feasibility — a verifier that cannot fail is worthless.

| Harness | Intent | Result |
| --- | --- | --- |
| `d1_add_number_can_decrease_the_counter` | must FAIL: adding can decrease the counter | ✅ 1 of 1110 checks failed, panic found as expected, 26s |
| `d2_add_number_is_exactly_wrapping` | pins the semantics: result `== a.wrapping_add(b)` | ✅ 25s |
| `d3_add_number_exact_when_no_overflow` | control: exact once overflow is assumed away | ✅ 34s |

`d1` is a genuine bug in the stock `cargo stylus new` template, found
automatically over the full 2^256 input space. `d2` proves it is precisely a
wrap rather than some other fault, and `d3` shows the guarded version is exact —
together they make the demo airtight rather than a single red line.

**Concrete playback works**, and is the demo asset. It needs the unstable flag:

```bash
cargo kani -Z concrete-playback --concrete-playback=print \
    --harness d1_add_number_can_decrease_the_counter
```

Kani emits a runnable `#[test]` carrying the 64 witness bytes (two `[u8; 32]`
draws). Decoded, the counterexample it found is:

```
a       = 115792089237316195420432434140994567471862513504418138526629138312939329028097
b       = 115792089237316195414155332405607886707005876980447656720081318813958861225983
a + b   >= 2^256, so it wraps to
result  = 115792089237316195411016781537914546325598405819225231207252873118985060614144
```

`result < a` — the counter went *down* after adding to it. That is the whole
pitch in one screenshot: a real bug in the stock template, an exact witness, and
a test you can paste into the repo, all from one 26-second command.

### Finding: mappings are viable but an order of magnitude slower

Measured 2026-09-08 on [`examples/proofs`](../examples/proofs), Kani 0.67.0,
solver time only (the dependency-tree compile is shared and cached).

**Scalar storage — seconds.**

| Harness | Time |
| --- | --- |
| `counter::starts_at_zero` | 10s |
| `counter::set_then_get_roundtrips` | 26s |
| `counter::roundtrip_holds_for_any_transaction_context` | 32s |
| `counter::add_number_is_exactly_wrapping` | 43s |
| `counter::increment_wraps_at_max` | 44s |
| `counter::add_number_can_decrease_the_counter` | 47s |
| `vault::ownership_cannot_be_claimed_twice` | 46s |
| `vault::owner_can_transfer_ownership` | 56s |
| `counter::add_number_is_exact_when_it_does_not_overflow` | 57s |
| `vault::only_owner_can_transfer_ownership` | 61s |

**Mappings — minutes, scaling with the number of accesses.**

| Harness | Mapping work | Time |
| --- | --- | --- |
| `vault::credit_then_read_roundtrips` | 1 account, 1 write | 181s |
| `vault::credit_can_silently_wrap` | 1 account, 2 writes | 491s |
| `vault::credit_checked_never_wraps` | 1 account, 2 guarded writes | 501s |
| `vault::distinct_accounts_do_not_alias` | 2 accounts, 2 guarded writes | **1064s** |
| `vault::total_tracks_the_sum_of_balances` | 2 accounts + conservation assertions | did not finish in 23 min |

14 of 15 harnesses verify. Only the heaviest — two-account conservation — has
not been seen to converge, and it was never given more than 23 minutes, so
"slow" is established but "intractable" is not.

**Three things follow.**

1. **A full symbolic transaction context is nearly free** — 32s against 26s.
   Use `SymbolicVM::new()` freely; `concrete_ctx()` is not the optimisation it
   looks like.
2. **Access control is cheap.** All three owner-gated proofs land under a
   minute, because they touch only scalar slots. Proposal property #2 is
   comfortably in reach.
3. **Mapping cost tracks the number of accesses, not just their presence** —
   181s for one write, ~500s for two, ~1064s for two accounts. That is roughly
   quadratic-looking, which fits the diagnosis below.

**Likely cause, not yet confirmed.** Mapping slot keys are *symbolic* `U256`
digests, so `SlotStore`'s linear scan performs up to `SLOTS` symbolic 256-bit
equality comparisons on *every* load and store; more accounts means both more
scans and more entries to scan against. The counter's slots are small concrete
numbers, where the same scan is trivial.

Levers, cheapest first:

1. Lower the default `SLOTS` from 16 — most single proofs touch a handful.
2. Two-tier slot store: concrete scalar slots in a small direct-indexed array,
   keccak-derived slots in a separate short list.
3. Narrow balances to `u64`-shaped values where the property is not about the
   256-bit boundary.
4. Have the oracle return digests with a concrete discriminator in the high bits
   so slot comparison can short-circuit. Weakens the model; needs care.

**Confirm before optimising.** Vary `SLOTS` alone and measure — the same staged
method that found the `TestVM` problem. This is the main open engineering
question and it gates how large a target is realistic.

### Superseded: the regex hypothesis

An earlier reading of this file blamed the `lazy_static` `Regex`es in
`stylus-core/src/sol.rs` for the slowdown. **That was wrong.** Their only
consumer, `is_sol_keyword`, is called from just two places:
`stylus-sdk/src/abi/export/mod.rs` (behind the `export-abi` feature, which we do
not enable) and a `stylus-proc` derive macro (compile-time only). Neither is
reachable from a proof harness. The SIMD noise in the first spike log was
hashbrown's SSE2 group probing (`simd_bitmask_impl::<i8, 16>` is a 16-lane
`_mm_movemask_epi8`), i.e. `HashMap` again — not `memchr` via `regex`.

Kept here as a caution: the first log looked like a regex problem and was not.
Locate cost cliffs with staged harnesses, not by reading dependency trees.
