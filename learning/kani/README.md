# Reading Kani's formulas

Tiny pure-Rust harnesses in [src/lib.rs](src/lib.rs), sized so the formula CBMC
generates is short enough to read. Verified 2026-09-09 against Kani 0.67.0 /
CBMC 6.8.0 / Z3 4.8.10. For the solver layer itself, see [../solver/](../solver/).

```bash
cargo kani -Z stubbing                  # all 5 harnesses, ~1 min
./dump.sh h1_nondet                     # -> /tmp/h1_nondet.smt2  (88 lines)
z3 hand/h1.smt2                          # the hand-written equivalent
```

Standalone crate (own `[workspace]`), zero dependencies, so it can't disturb the
real build. `-Z stubbing` is needed because two harnesses use `#[kani::stub]`.

## Two tracks, and why you want both

**Track A — read what CBMC generates** (`./dump.sh`). Shows the *encoding*: how
a Rust harness becomes SSA, where the free variables come from, how properties
are represented. But it is not a query you can interpret directly:

> The dumped formula is `sat` even when the harness verifies. CBMC keeps each
> property as a Boolean variable (`B0`, `B1`, …) and drives them itself rather
> than asserting them. `h1_nondet` verifies; `z3 /tmp/h1_nondet.smt2` says `sat`.

**Track B — write the query by hand** ([hand/](hand/)). The same properties in
clean `unsat = proved` form, three to twelve lines each. This is where you get a
feel for what the solver is actually being asked.

| harness | hand-written pair | verdict |
| --- | --- | --- |
| `h1_nondet` | [hand/h1.smt2](hand/h1.smt2) | `unsat` — proved |
| `h2_add_guarded` | [hand/h2.smt2](hand/h2.smt2) | `unsat`, `unsat`, `sat` |
| `h3_stub_*` | [hand/h3.smt2](hand/h3.smt2) | `unsat`, `unsat`, `sat` |

## Walking through h1_nondet (88 lines)

Four things to notice, all of which recur in every formula Kani produces.

**1. Exactly one free variable.** Everything else is a `define-fun` alias.

```smt2
(declare-fun |nondet_symex::nondet0| () (_ BitVec 8))
```

**2. `kani::any()` is a chain of pass-throughs.** The value flows
`any_raw_internal` → `Arbitrary::any` → `kani::any` → `a`, and each hop is one
`define-fun` equating the next name to the previous. That's SSA, not
computation — the `!0@1#2` suffixes are CBMC's SSA versioning.

**3. `bool` is eight bits wide.** Rust's `bool` lowers to `(_ BitVec 8)`, so
conditions arrive as `ite(…, bv1, bv0)` and get tested with `= … bv0`. The
double-negation chains (`var_5` ← `var_6` ← `var_7`) are MIR lowering artifacts,
not something you wrote.

**4. Properties are variables, not assertions.**

```smt2
(define-fun B0 () Bool (not (= |…var_3…| (_ bv0 8))))      ; the assume holds
(define-fun B1 () Bool (=> (and true B0) false))            ; reachability
(define-fun B2 () Bool (=> (and true B0) (not (= …))))      ; assert!(a < 10)
(assert (or (not B1) (not B2)))                             ; "some property fails"
```

`B2` is the real property. `B1` is the assertion-*reachability* check Kani asks
for with `--assertion-reach-checks`; its `… => false` shape fails exactly when
the location is reachable, which is why a passing harness still yields `sat`.

## Where the automatic overflow check comes from

`h2_add_guarded` reports **2 of 2** properties for one `assert!`. The extra one
is the panic branch rustc inserts for `a + b`, because Kani compiles with
`-C overflow-checks=on`. Its formula is 205 lines with 2 free variables.

[hand/h2.smt2](hand/h2.smt2) makes the trap explicit: you cannot say
"overflowed" in the width you overflowed out of — `bvugt (bvadd a b) #xff` is
unsatisfiable for every 8-bit input, because `bvadd` already wrapped. Widen
first. This is the same trap as the KB's rule that
[U256 arithmetic does not trap](../../kb/50-feasibility.md).

## Stubbing, at the formula level

`mask(x) = x ^ 0x5a`, so `mask` costs exactly one greppable SMT operator. All
three harnesses assert `mask(mask(x)) == x`, which is true.

| harness | stub | `bvxor` | free 8-bit decls | result |
| --- | --- | --- | --- | --- |
| `h3_stub_none` | — | **2** | 1 | verifies |
| `h3_stub_equivalent` | `mask_identity` | **0** | 1 | verifies |
| `h3_stub_any_overapproximates` | `mask_any` | **0** | **4** | **fails** |

A stub **deletes a subformula**. That's the whole mechanism:

- The unstubbed formula contains `mask`'s body twice, inlined per call site and
  distinguished only by SSA instance (`@1#1` vs `@2#2`):
  ```smt2
  (define-fun |…mask::1::var_0!0@1#2| () (_ BitVec 8) (bvxor |…mask::1::var_1::x!0@1#1| (_ bv90 8)))
  (define-fun |…mask::1::var_0!0@2#2| () (_ BitVec 8) (bvxor |…mask::1::var_1::x!0@2#1| (_ bv90 8)))
  ```
- `mask_identity` removes them and preserves the property. Sound, and cheaper.
- `mask_any` removes them and puts **unconstrained variables** in their place
  (declaration count goes 1 → 4, including CBMC's `symex::args::` entries for
  the arguments the stub ignores). The two results are now unrelated, so the
  solver returns a counterexample to code that is correct.

That last one is the point of the exercise. Replacing a body with
`kani::any()` is an **over-approximation**: it admits behaviours the real code
never has, so you get false positives — noisy, but safe. The dangerous
direction is the opposite one: a stub *stronger* than the code it replaces makes
proofs succeed for reasons the real code doesn't support. **Kani does not check
that a stub refines what it replaces.** Every stub is a proof obligation you
have taken on yourself, discharged by review or not at all.
