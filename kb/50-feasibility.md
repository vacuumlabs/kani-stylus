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
  this shape, already used in `examples/counter/src/lib.rs`.

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

Ordered by how much they gate the plan. Answers below were established by the
experiments recorded later in this file; each says how it was verified.

1. ~~**Does `cargo kani` complete a build against `stylus-sdk --features
   stylus-test`, and can the dependency surface be pruned enough for proofs to
   converge?**~~ — **Answered 2026-09-08: yes, by not using `TestVM`.** The
   268-crate tree (tokio/reqwest included) costs compile time but never enters
   the goto program as long as nothing constructs a `TestVM`. Verified by the
   two example suites: 14 of 14 harnesses verify. The regex hypothesis was
   wrong — see "Superseded" at the end of this file.
2. ~~**Does dynamic dispatch through `Box<dyn Host>` blow up the encoding?**~~ —
   **Answered 2026-09-08 in practice: it is not the bottleneck.** Scalar proofs
   land in 6–90s with the box in place. Never isolated as its own experiment, so
   this is "not the binding constraint" rather than "free"; the measured
   bottleneck is symbolic mapping keys in `SlotStore` (question 8).
3. ~~**How do we model keccak256?**~~ — **Answered 2026-09-08: an uninterpreted
   injective function**, implemented in
   [`crates/kani-stylus-core/src/keccak.rs`](../crates/kani-stylus-core/src/keccak.rs).
   An 8-entry preimage/digest table; a new preimage mints a fresh symbolic
   digest, `assume`d distinct from every previous digest and `assume`d above
   slot 2^32. Needs `-Z stubbing` and an explicit `#[kani::stub]` per harness,
   because `stylus_sdk::crypto::keccak` bypasses the `Host` trait. Mapping
   proofs verify; the assumption is disclosed in the crate README.
4. ~~**Do `U256` operations converge?**~~ — **Answered 2026-09-08: yes, at full
   width.** `counter::mul_number_can_wrap` 80s and
   `add_number_is_exact_when_it_does_not_overflow` 65s, both over fully symbolic
   `U256`. Narrowing to `u64` shapes remains a useful lever, not a necessity.
5. **Is OpenZeppelin `rust-contracts-stylus` ERC-20 tractable, or do we need our
   own minimal ERC-20?** **Still open, and now the most consequential question
   here.** Neither has been attempted; `examples/vault` is a hand-rolled
   stand-in that exercises mappings but is not a real ERC-20. Gated by
   question 8.
6. ~~**`rust-toolchain.toml` interaction.**~~ — **Answered 2026-09-08: not a
   problem.** `cargo kani` runs fine inside a crate pinning its own toolchain.
   See [40-toolchain.md](40-toolchain.md).
7. **Does `#[public]`/`sol_storage!` macro-generated code verify cleanly?**
   **Partially answered 2026-09-08.** Method-level proofs verify cleanly against
   `sol_storage!`-generated storage accessors in both examples. Proving *through*
   the ABI router from raw calldata — which is what "panic freedom over arbitrary
   calldata" actually requires — remains unattempted and is strictly harder. The
   proposal conflates the two; say method-level explicitly in any writeup.
8. **Can mapping proofs be made cheap enough for multi-account properties?**
   Raised 2026-09-09 and **immediately downgraded from "gating" the same day.**
   Conservation turned out not to need it — reformulating as local-delta lemmas
   converges today at 463–1904s per harness (see "Conservation by local deltas").
   What remains is pace and reach: half an hour per harness makes iteration
   miserable, and 6.4–10 GiB per `cbmc` caps how many accounts fit. Diagnosis
   and the remaining levers are in the mappings section; confirm by varying
   `SLOTS` alone before optimising.
9. **How do we express properties over *sequences* of calls?** Raised
   2026-09-09. Every harness today proves one method call from a hand-havoc'd
   state, but the properties contract authors want ("no sequence of calls
   breaks this") need either an inductive invariant — base case plus a step case
   from arbitrary state satisfying the invariant — or a bounded symbolic-action
   dispatcher. Neither is prototyped, and there is no `SymbolicVM::havoc()` to
   build the arbitrary pre-state conveniently.

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

Established with a throwaway crate: a `sol_storage!` counter plus staged
harnesses adding one layer at a time, so the cost cliff could be located rather
than guessed at. The crate is gone (it became
[`crates/kani-stylus-core`](../crates/kani-stylus-core) and the examples); the
numbers and the reproduction recipe are below, and the original is in git
history at `spikes/kani-smoke`.

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

<details>
<summary>Reproducing it (this is the project's load-bearing claim, so it should
stay checkable)</summary>

In any Stylus crate with `stylus-sdk/stylus-test` enabled, add:

```rust
#[cfg(kani)]
#[kani::proof]
fn testvm_cannot_be_verified() {
    let vm = stylus_sdk::testing::TestVM::default();
    core::hint::black_box(&vm);
}
```

`cargo kani --harness testvm_cannot_be_verified` fails within seconds on the
`getrandom` syscall. It is deliberately **not** part of the committed suite: it
fails with an unsupported-construct error rather than a panic, so
`#[kani::should_panic]` cannot absorb it and it would turn `./verify.sh` red.

</details>

### Result: a purpose-built symbolic host works, and is fast

The prototype — now [`crates/kani-stylus-core`](../crates/kani-stylus-core) —
implements `stylus_core::Host` directly: storage is a fixed-size array with a
linear scan, transaction context is drawn once at construction, and unmodelled
operations (`create1`, `call_contract`, …) are `unimplemented!()` so that a
proof touching them fails loudly instead of silently.

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
  `mul_number` in `examples/counter` have no overflow protection.
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

### Result: it works in an unmodified `cargo stylus new` project

The point of the tool is that a Stylus developer adds it to the project they
already have. Verified 2026-09-08 on
[`examples/counter`](../examples/counter) — the stock template, with
proofs added to `src/lib.rs` beside its existing `#[cfg(test)]` module and three
lines of `Cargo.toml`. No restructuring, no separate crate.

**7 of 7 harnesses verify, 380s for the suite:**

| Harness | Time |
| --- | --- |
| `starts_at_zero` | 12s |
| `set_then_get_roundtrips` | 39s |
| `add_from_msg_value_adds_exactly_the_value_sent` (symbolic `msg_value`) | 49s |
| `increment_wraps_at_max` | 50s |
| `add_number_can_decrease_the_counter` | 55s |
| `add_number_is_exact_when_it_does_not_overflow` | 65s |
| `mul_number_can_wrap` | 80s |

**The ordinary workflow is provably unaffected:**

| Command | Result |
| --- | --- |
| `cargo test` | passes — the template's own `test_counter` |
| `cargo build --target wasm32-unknown-unknown --release` | 18.5 KB cdylib; `strings` shows no `kani` symbols and no `stylus-test` panic stub, and the real `vm_hooks` imports are intact |
| `cargo stylus check` | compiles and sizes the contract at 6.0 KB. Its activation step needs a Stylus RPC (defaults to `localhost:8547`); against a devnode it reported a wasm data fee of 0.000071 ETH, and offline it stops after the size report |
| `cargo kani --features proofs` | the 7 harnesses above |

See [20-stylus.md](20-stylus.md#packaging-how-verification-attaches-to-a-real-contract)
for why the feature gate is mandatory rather than stylistic.

**Three wrapping methods, not one.** Verifying the real template rather than a
copy surfaced that `add_number`, `mul_number` *and* `increment` all wrap
silently. The earlier hand-copied contract omitted `mul_number` and
`add_from_msg_value` entirely — a good argument for pointing the tool at real
code rather than a convenient subset.

### Finding: mappings are viable but an order of magnitude slower

All numbers measured 2026-09-08 on the two example projects, Kani 0.67.0,
solver time only (the dependency-tree compile is shared and cached).
**14 of 14 harnesses verify** — 7 in `examples/counter` (380s total), 7 in
`examples/vault` (3155s total). Superseded in part: `examples/vault` gained
three conservation harnesses on 2026-09-09, verified individually but not yet
in a clean suite run. See "Conservation by local deltas".

**Scalar storage — seconds.**

| Harness | Time |
| --- | --- |
| `counter::starts_at_zero` | 12s |
| `counter::set_then_get_roundtrips` | 39s |
| `counter::add_from_msg_value_adds_exactly_the_value_sent` | 49s |
| `counter::increment_wraps_at_max` | 50s |
| `counter::add_number_can_decrease_the_counter` | 55s |
| `counter::add_number_is_exact_when_it_does_not_overflow` | 65s |
| `vault::ownership_cannot_be_claimed_twice` | 68s |
| `vault::only_owner_can_transfer_ownership` | 72s |
| `counter::mul_number_can_wrap` | 80s |
| `vault::owner_can_transfer_ownership` | 88s |

**Mappings — minutes, scaling with the number of accesses.**

| Harness | Mapping work | Time |
| --- | --- | --- |
| `vault::credit_then_read_roundtrips` | 1 account, 1 write | 238s |
| `vault::credit_can_silently_wrap` | 1 account, 2 writes | 561s |
| `vault::credit_checked_never_wraps` | 1 account, 2 guarded writes | 675s |
| `vault::distinct_accounts_do_not_alias` | 2 accounts, 2 guarded writes | 1236s |

**Three things follow.**

1. **Access control is cheap.** Every owner-gated proof lands under 90s, because
   they touch only scalar slots. Proposal property #2 is comfortably in reach.
2. **Mapping cost tracks the number of accesses**, not merely their presence:
   238s → 561s → 1236s. Roughly quadratic-looking, which fits the diagnosis
   below.
3. **Symbolic context is cheap on scalars, not on mappings.** Earlier
   measurements with a concrete context gave 181s / 491s / 501s / 1064s for the
   four mapping proofs above; switching them to a fully symbolic
   `SymbolicVM::new()` cost **+15% to +35%**. On scalar proofs the same change
   costs ~6s. So "use `new()` freely" holds for scalar properties; on
   mapping-heavy ones, `concrete_ctx()` is a real lever when the property does
   not depend on the caller.

**Multi-account conservation — superseded 2026-09-09.** A harness asserting
`total == balance(a) + balance(b)` across two accounts was not seen to converge
in 23 minutes. That harness has been **abandoned rather than optimised**: the
summed form is the wrong thing to ask a bounded model checker for. See
"Conservation by local deltas" below, which gets the property another way and
converges today.

**Likely cause — tested 2026-09-09 and WRONG about time.** The standing
hypothesis was that `SlotStore`'s linear scan of up to `SLOTS` symbolic 256-bit
keys drives the cost. It drives *formula size* but not *solve time*: see
"`SLOTS` is a memory dial, not a time dial" below. What actually makes mapping
proofs slow is still unknown.

Of the four levers once listed here, two were **measured on 2026-09-09 and do
not help with time** — lowering `SLOTS`, and giving digests a concrete high-bit
discriminator. Two remain untested: a two-tier slot store (concrete scalar slots
direct-indexed, keccak slots in a short list), and narrowing balances to
`u64`-shaped values. See the findings that follow.

### Result: conservation by local deltas (2026-09-09)

`total == sum of every balance` **cannot be stated in a bounded model checker**:
there is no quantifying over 2^160 addresses, and the summed two-account form
above never converged. Asking for it directly was the mistake.

Decomposed instead. For each method, prove *locally*, from an **arbitrary**
pre-state, that (a) it moves `total` by exactly the net amount it moves balances
by, and (b) it changes nothing else — the **frame condition**. Global
conservation then follows by induction over any call sequence.

**The induction step is a hand argument, not machine-checked.** Kani proves the
per-method lemmas; composing them over sequences is on paper. This is the
standard decomposition — it is what Certora rules do for Solidity — but it must
be stated rather than implied. Machine-checking the composition is roadmap
item 2 (see [60-roadmap.md](60-roadmap.md)).

`examples/vault` gained a `transfer` method (checked throughout; rejects
self-transfer, since `from == to` with naive read-modify-write is a standard way
to mint from nothing) and three lemmas. All verify, measured one at a time:

| Harness | Shape | Checks | Time |
| --- | --- | --- | --- |
| `credit_checked_moves_total_by_the_same_delta` | 1 account, 2 slots | 1448 | ✅ 463s |
| `transfer_conserves_total` | 2 accounts, 3 slots | 1501 | ✅ 1836s |
| `transfer_does_not_move_any_other_balance` | 3 accounts | 1457 | ✅ 1904s |

The first two report `1 of 1 cover properties satisfied`, so the expected slot
counts are reachable and neither proof is vacuous — worth checking, because they
run at `SymbolicVm::<4>` rather than the default 16.

**Two ways to write the frame condition**, both kept in the example so the
trade-off stays visible:

1. **Count slots** — new API `vm.snapshot()` / `vm.slots_changed_since(&s)`,
   backed by `SlotStore::changed_since`. Cheap and needs no knowledge of slot
   derivation, but bounds only *how many* slots moved, not which. Note the
   assertion is `<=`, not `==`: a frame condition is an upper bound, and a
   zero-amount call changes nothing.
2. **A symbolic third party** — `other` assumed distinct from both parties, so
   `unsat` covers every remaining address at once. This is the universal
   quantifier a BMC gives you for free, and it is what
   `transfer_does_not_move_any_other_balance` uses. Strictly stronger, and it
   costs a third mapping account (1904s vs 1836s — cheaper than expected).

**`changed_since` costs up to `SLOTS^2` symbolic 256-bit comparisons**, which is
why these harnesses drop to `SymbolicVm::<4>`. That drop was *not* measured in
isolation, so its contribution to the times above is unknown.

**Use `checked_add`/`checked_sub` in the assertions, not just the contract.**
Bare `+` on `U256` wraps, so `assert_eq!(after, before + amount)` proves
something weaker than intended. The lemmas above assert
`before.checked_add(amount).unwrap()`, which is provable precisely because the
method returned `Ok`.

### Finding: `SLOTS` is a memory dial, not a time dial (2026-09-09)

The long-standing hypothesis — that `SlotStore`'s scan of up to `SLOTS` symbolic
256-bit keys is what makes mapping proofs slow — is **wrong about time**.
Measured on `vault::credit_then_read_roundtrips`, varying `SLOTS` alone,
one sample per point on an otherwise idle machine:

| `SLOTS` | CNF variables | CNF clauses | solve time |
| --- | --- | --- | --- |
| 2 | 725,615 | 1,905,301 | 157s |
| 4 | 860,521 | 2,454,434 | 196s |
| 8 | 1,154,931 | 3,646,905 | 185s |
| 16 (default) | 1,842,077 | 6,408,226 | 192s |

Formula size varies **2.5x**; solve time is flat and not even monotonic. The
clauses contributed by unused slots are evidently dispatched by unit propagation
without adding search. Size grows linearly — `vars ≈ 566k + 80k × SLOTS` — so
roughly 566k variables are irreducible (SDK, contract, oracle).

Caveat: one harness, one sample per point. It rules out a *large* effect, not a
10% one.

**`SLOTS` still matters, for memory.** Size decides whether a proof can be
encoded at all, and memory is the binding constraint (next section).
`transfer_conserves_total` needs 6,624,296 vars / 26,361,004 clauses at
`SLOTS=4` and **could not be encoded within 5 GiB at `SLOTS=8` or `16`**. So the
conservation lemmas' `SymbolicVm::<4>` is load-bearing — but for encodability,
not speed.

Two other size levers were measured on the same harness and are **not worth
taking**: `MAX_HASHES` 8→4→2 changes the formula by ~1% (the oracle's loops run
to `self.len`, the actual number of hashes, not to the bound), and giving
digests a fixed high prefix so only 64 bits stay symbolic buys ~7%.

**Methodological warning.** Formula size was used as a fast proxy for solver
cost — `--cbmc-args --dimacs --outfile` skips solving, turning a 30-minute loop
into 2 minutes. The proxy is **invalid** for this workload, as the table shows.
It is still the right tool for predicting *memory* and encodability. Rank
time optimisations by real solves only.

**Solver choice buys ~10% at best** — measured the same day on the same
harness, shipped config:

| `--solver` | Time |
| --- | --- |
| `cadical` (Kani's default) | 217s |
| `kissat` | 194s |
| `minisat` | 199s |
| `z3`, `cvc5`, `bitwuzla` | unusable here |

All three SAT backends land within noise of each other, so the instance is not
sensitive to CDCL heuristics. The SMT backends are a tooling failure, not a
disagreement: `z3` 4.8.10 cannot digest CBMC 6.8's SMT2 and CBMC exits with
status 6, which Kani surfaces as `VERIFICATION:- FAILED`. **That is not a
soundness signal** — worth knowing before someone reads it as one. `cvc5` and
`bitwuzla` are not installed.

**What actually drives solve time is still unknown.** Remaining untested
candidates: narrowing `U256` values to `u64` shapes, which shrinks the search
space rather than the formula; the two-tier slot store.

### Finding: memory, not time, is the binding constraint on mapping proofs

Measured 2026-09-09, and it cost an editor to learn. Two mapping proofs run
**concurrently** from the VSCode integrated terminal exhausted a 23 GiB machine:
one `cbmc` reached **6.4 GiB resident** and the global OOM killer shot it. Because
terminal children inherit the editor's systemd scope, systemd tore down the whole
`app-code-*.scope` — killing VSCode. Full journal excerpt and the
`systemd-run --user` recipe that avoids it are in
[40-toolchain.md](40-toolchain.md).

Consequences for planning:

- **Mapping proofs are effectively serial on a laptop**, regardless of core
  count. Every timing in this file was measured one-at-a-time and none of them
  parallelise.
- Peak RSS for one mapping harness is **between 6.4 and 10 GiB** — the lower
  bound from the OOM kill, the upper from all three lemmas later completing
  under a `MemoryMax=10G` cap. It is not pinned more precisely than that: two
  attempts to measure it failed, because `/usr/bin/time -v` wrapped
  `systemd-run` rather than `cbmc` (reporting 7.4 MB, the wrapper's own
  footprint) and systemd's own `Memory peak` reported 328 K for the same reason.
  Poll `/proc/<cbmc-pid>/status` if the exact figure is ever needed.
- `systemd-run --wait` sends unit stdout to **the journal**, not the terminal.
  Read verdicts with `journalctl --user -u kani-<harness>`.

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
