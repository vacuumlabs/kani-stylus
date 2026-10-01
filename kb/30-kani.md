# How Kani works (and where it stops)

Sourced from the [Kani book](https://model-checking.github.io/kani/) on
2026-09-08 against the locally installed **Kani 0.67.0**.

## The model

Kani is a **bounded model checker**, not a proof assistant. It compiles Rust to
CBMC's goto-IR, encodes the program plus its assertions as an SMT formula, and
asks a solver whether any input violates them. Three outcomes: property holds
(within the bound), property fails with a **concrete counterexample**, or the
solver runs out of resources (`UNDETERMINED`).

The counterexample is the reason this is worth doing for smart contracts. Unlike
a fuzzer, Kani hands you the exact `(sender, amount, balance)` triple that
breaks your invariant.

## The API surface we'll use

```rust
#[kani::proof]                 // marks a harness; `cargo kani` discovers these
#[kani::unwind(N)]             // bound on loop iterations / recursion depth
#[kani::solver(cadical)]       // pick the SAT/SMT backend
#[kani::stub(orig, replace)]   // swap a function out during verification
#[kani::should_panic]          // the harness is expected to panic

kani::any::<T>()               // fresh symbolic value; needs T: kani::Arbitrary
kani::assume(cond)             // constrain the input space (a precondition)
kani::assert(cond, "msg")      // a property to prove
kani::cover(cond)              // assert some path *can* reach this state
```

By default Kani checks, on top of your assertions: arithmetic overflow, array
bounds, null/misaligned pointer dereference, division by zero, and reachable
`panic!`/`unwrap`. For a contract, "no assertion, just run it" already proves
panic freedom over the whole symbolic input space — that's proposal property #3
essentially for free.

Driving it:

```bash
cargo kani                            # every harness in the crate
cargo kani --harness <name>           # one harness
cargo kani --features stylus-test     # feature flags pass through
cargo kani --output-format terse
cargo kani --concrete-playback=print  # emit a replayable #[test] from a counterexample
```

`--concrete-playback` is a strong demo asset: it turns a solver counterexample
into a runnable Rust test.

## Limits that will bite this project

**Bounded, not universal.** `#[kani::unwind(N)]` caps loop iterations. Proofs
hold only up to that bound. Kani reports an unwinding-assertion failure if `N`
is too small, so it fails loud rather than silently under-checking — but any
claim we make in the writeup must say "for up to N iterations".

**Concurrency is out.** "Kani focuses on sequential code." Fine for us —
contract execution is sequential.

**Inline assembly is unsupported.** Worth grepping for in dependencies.

**Undefined behaviour Kani does *not* detect** (from
[undefined-behaviour.html](https://model-checking.github.io/kani/undefined-behaviour.html)):
data races; pointer-aliasing violations (it catches misuse that causes a memory
safety or assertion failure, but "does not track reference lifetimes");
mutation of immutable data; some compiler-intrinsic preconditions
("best effort attempt … does not guarantee to do so in all cases"). And the
overall proviso: *"verification results are subject to the proviso that the
program under verification does not contain UB."*

**Invalid values via transmute.** Kani "won't create invalid values with
`kani::any()` but it also won't complain if you transmute an invalid value to a
Rust type."

**Solver blowup is the real risk.** 256-bit arithmetic (`U256` everywhere in
Stylus), keccak256, and dynamic dispatch through `Box<dyn Host>` all inflate the
formula. Mitigations, roughly in order of what to reach for:

1. Model keccak256 as an uninterpreted injective function rather than its bits.
2. Keep the symbolic storage map tiny (8–16 slots).
3. Constrain inputs hard with `kani::assume` — e.g. balances bounded well below
   `U256::MAX` when the property doesn't need the full range.
4. Prove over `u64`-shaped values first, widen to `U256` once a proof converges.
5. Try alternative solvers via `#[kani::solver(...)]`.
6. `#[kani::stub]` out anything irreducibly expensive.

Always check the [Rust feature support](https://model-checking.github.io/kani/rust-feature-support.html)
table before assuming a construct verifies.

**Stubbing has limits, and they decide where a model can hook in.** Measured
on Kani 0.67.0, 2026-09-30: generic trait methods cannot be stubbed
([#1997](https://github.com/model-checking/kani/issues/1997)), methods of
generic impls crash the compiler, and about fourteen Kani attributes on one
function exhaust rustc's recursion limit. Details and what worked instead:
[36-storage-model.md](36-storage-model.md#kani-limits-that-shaped-this).

**Assertion-reachability checks can dominate the run.** They are on by
default, and on a small harness most of the reported verification time went on
turning their traces into results, not on CBMC: 49s with them, 10.5s
without, at half the memory (2026-09-30). `--no-assertion-reach-checks` plus
explicit `kani::cover!`s is the cheaper way to iterate; see
[36-storage-model.md](36-storage-model.md#method-notes).

## Experimental features that may be useful later

- [Function contracts](https://model-checking.github.io/kani/reference/experimental/contracts.html) —
  `requires`/`ensures` for modular verification. Would let us verify a contract
  method against a spec and then *reuse* that spec, instead of re-verifying the
  body at each call site. Interesting for scaling past the MVP.
- [Loop contracts](https://model-checking.github.io/kani/reference/experimental/loop-contracts.html) —
  loop invariants that turn bounded proofs into unbounded ones. Relevant for
  batch-transfer style methods.
- [Concrete playback](https://model-checking.github.io/kani/reference/experimental/concrete-playback.html) —
  counterexample → executable test.

They're experimental; treat them as stretch goals, not MVP dependencies.
