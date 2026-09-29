# Nonlinear arithmetic: exact division, and the arithmetic oracle

Verified against Kani 0.67.0, CBMC 6.8.0, ruint 1.16.0, stylus-sdk 0.10.9.
Measurements are dated where they appear. The code is
[`arith.rs`](../crates/kani-stylus-core/src/arith.rs) (exact division) and
[`arith_oracle.rs`](../crates/kani-stylus-core/src/arith_oracle.rs) (the
oracle); the worked examples are in [`examples/vesting`](../examples/vesting/src/lib.rs).

## The problem

Business logic on `U256` is full of `x * y / z`: shares, rates, vesting, fees.
Left alone, ruint's `/` does not verify at all: its loops run over slices whose
length CBMC cannot bound, and symbolic execution never ends (details in
[`arith`](../crates/kani-stylus-core/src/arith.rs)'s module docs). Replacing it
with its exact specification — `r < b` and `a == q * b + r` at 512 bits — fixes
that. It does not fix the second, deeper cost: **nonlinear arithmetic is exactly
what SAT cannot do at width.** Native primitive division shows it with no
library involved (2026-09-24):

| `a1 <= a2 && b != 0  ⇒  a1 / b <= a2 / b` | Solver time |
| --- | --- |
| `u8` | 0.2s |
| `u16` | 80s |
| `u32` | > 300s (timeout) |
| `u64`, divisor fixed at 100 | 5.9s |

Changing the solver does not rescue it: at `u16`, Bitwuzla took 65s, kissat
93s, cvc5 122s and CBMC's own `--refine-arithmetic` 115s, against CaDiCaL's 80s.
The exact specification inherits the problem: the same property at `U256`
still times out at 300s.

## Two models, for two jobs

| | [`arith`](../crates/kani-stylus-core/src/arith.rs) — exact | [`arith_oracle`](../crates/kani-stylus-core/src/arith_oracle.rs) — abstract |
| --- | --- | --- |
| Replaces | `/` | `*` and `/` |
| A counterexample is | always real | possibly spurious |
| A proof resting on nonlinear facts | as expensive as ever | cheap |
| Use it for | demonstrating bugs; every `#[kani::should_panic]` harness | proving properties |

The `should_panic` rule is not a style preference. Kani's `should_panic`
"verifies that there are one or more failed checks related to panics. At the
moment, it's not possible to pin it down to specific panics", so a spurious
counterexample would pass the harness for the wrong reason.

Certora and hevm split the work the same way — abstract to prove, exact to check
a counterexample; see [Prior art](#prior-art-this-is-how-other-smart-contract-provers-do-it).

## How the oracle works

Contracts almost never need the *bits* of a product. A vesting proof needs "a
product grows when an operand grows", "a quotient grows when the numerator
grows", and "`total * elapsed / duration` is at most `total` while
`elapsed <= duration`". Those are the facts to give the solver — not the
multiplier circuit they follow from.

So `arith_oracle` replaces `*` and `/` with **uninterpreted functions
constrained by lemmas**, the same move the
[keccak oracle](../crates/kani-stylus-core/src/keccak.rs) makes for hashing.
Each operation returns a fresh symbolic value. It is recorded in a small memo
table, and the stub assumes lemmas about it:

- about the operation alone — `x * 0 == 0`, `x * 1 == x`, `a / b <= a`,
  `a < b ⇒ a / b == 0`;
- about it and every earlier operation of the same kind — equal inputs give
  equal outputs, `*` is commutative, both are monotone;
- about a division and an earlier multiplication whose result it divides —
  `y <= z ⇒ x * y / z <= x`, `x * y / y == x`.

Lemmas that could be wrong under wrapping carry a no-wrap premise,
`bit_len(a) + bit_len(b) <= 256`. The solver then only compares opaque 256-bit
values, which is cheap at any width.

**A Kani function contract cannot express this.** An `ensures` clause relates
one call's result to that call's own inputs. Monotonicity relates *two* calls,
and the mulDiv bound relates a division to a multiplication. The memo table is
what makes such relational lemmas stateable; it is also why the keccak oracle
has one, since injectivity is relational too.

`arith`'s `wrapping_div_stub_monotone` sits between the two: the exact
specification plus the division-monotonicity lemma as a hint. The hint is
entailed, so counterexamples stay real — but the 512-bit multiplier is still in
the formula. Measured on a prototype of the same idea (2026-09-24): exact
division plus hints took division monotonicity from 5s (oracle) to 226s, and
the vesting property timed out at 300s, as did *finding* its deliberate wrap
bug. The multiplier circuit is the cost, whether or not the proof needs it.

## Using the oracle

```rust
#[kani::proof]
#[kani::stub(ruint::Uint::wrapping_mul, kani_stylus_core::arith_oracle::mul_stub)]
#[kani::stub(ruint::Uint::wrapping_div, kani_stylus_core::arith_oracle::div_stub)]
fn vested_is_monotone_in_time() { /* ... */ }
```

with `cargo kani -Z stubbing`. `*`, `/`, `*=`, `/=` and `checked_div` route
through the two stubbed functions. `checked_mul`, `overflowing_mul` and `%` do
not, and stay bit-precise. `+` and `-` are left bit-precise on purpose: adders
are linear and cheap to verify at 256 bits.

## What an oracle result means

- **A pass is sound, if every lemma is true of real arithmetic.** That is the
  whole trust argument, and it is testable: each lemma is a pure predicate, and
  `cargo test -p kani-stylus-core` checks every one against real ruint, on
  20,000 generated rounds per lemma family, built so each premise actually
  fires. Planting a false lemma — monotonicity without the no-wrap premise, or
  the mulDiv bound reversed — makes those tests fail, so they have teeth.
- **A failure may be spurious.** The oracle knows only its lemmas; it does not
  even know that `3 * 5 == 15`. While developing this, a first version of the
  vesting proof failed with `t2 == start + duration`: the contract returns
  `total` directly on that branch, and without the mulDiv bound the solver was
  free to make the other branch's `total * e / d` exceed `total`. Replayed on
  real arithmetic, the counterexample held no bug. **Always replay a
  counterexample on real arithmetic before reporting it.** Kani's concrete
  playback runs *without* stubs, which is nearly what is needed, but it warns
  that the stream of symbolic values misaligns when a stub draws values of its
  own — which these stubs do. They stay aligned as long as nothing else draws a
  symbolic value after the first `*` or `/`.
- **Check it is not vacuous.** Both vesting proofs end with a `kani::cover!`
  confined to the branch that multiplies and divides. A satisfied cover shows
  the oracle's lemmas are consistent there — for the monotonicity proof, that
  vesting can still grow strictly mid-schedule, so the reverse inequality is not
  provable. A cover over the early-return branches would prove nothing about the
  oracle, since no `*` or `/` runs there.
- **Bounded, but never vacuous.** The oracle records up to `MAX_OPS` (8)
  multiplications and as many divisions. Operations beyond that still get every
  lemma against every recorded one, but are not recorded themselves. That only
  drops true facts: it can make a proof fail spuriously, but it never prunes a
  path and never panics — so it cannot make a `should_panic` harness pass, and
  it cannot make a proof vacuous.
- **`U256` only.** A stub replaces a function at *every* width, so a harness
  that reaches `U64 * U64` fails to compile rather than being mis-modelled.

## Measured

Measured 2026-09-29 with `verify.sh`'s flags, one cgroup and one run each,
verification time as Kani reports it. The exact-arithmetic rows are Martin's
own status for them, not re-measured here.

| Harness | Model | Result | Time | Peak RSS |
| --- | --- | --- | --- | --- |
| `vested_is_monotone_in_time` | exact | marked "DOES NOT CONVERGE" even with `start`, `duration` and `total` fixed | — | — |
| `vested_is_monotone_in_time` — every `t1 <= t2`, all parameters symbolic, `total` up to 2^192, through storage | oracle | **SUCCESSFUL**, 0 of 1748 failed, cover satisfied | 376s | cbmc 1.8 GiB, kani-driver 8.0 GiB |
| `vested_never_exceeds_total` | exact | behind `slow-proofs` | — | — |
| `vested_never_exceeds_total` | oracle | **SUCCESSFUL**, 0 of 515 failed, cover satisfied | 12s | cbmc 164 MiB |

**The memory is not the solver's.** With Kani's default assertion-reachability
checks, `kani-driver` — which processes CBMC's results — peaks at about 8 GiB on
the monotonicity proof while `cbmc` itself stays under 2 GiB. Without them
(measured 2026-09-24 on an earlier version of the proof) `kani-driver` needed
19 MiB and the proof ran in a third of the time. `verify.sh` keeps the checks,
since they are what catch a vacuous proof — but mind the machine: a full
`./verify.sh vesting` inside a 10 GiB cgroup still made a 23 GiB laptop
unusable, because the cap stops an OOM from spreading, not machine-wide
memory pressure. Size the cap to what is actually free, and see
[40-toolchain.md](40-toolchain.md). Whether the same effect is behind the
vault's 6–10 GiB mapping proofs has not been tested.

## Prior art: this is how other smart-contract provers do it

- **Certora Prover** — the closest match, and in production. Hozzová, Bendík,
  Nutz, Rodeh, [*Overapproximation of Non-Linear Integer Arithmetic for Smart
  Contract Verification*](https://easychair.org/publications/paper/BlrQ), LPAR
  2023 ([PDF](https://easychair.org/publications/download/BlrQ)) — three of
  the four authors at Certora, describing "the two overapproximation techniques
  used by the industry verification tool Certora Prover". It replaces `*`, `div`
  and `mod` with uninterpreted functions plus axioms, including "relating pairs
  of multiplications: monotonicity and distributivity", which "are
  instantiatiated [sic] over single applications of the operators, as well as
  pairs of multiplications that lie on a common program path". It is "designed
  for proving the VC unsatisfiability, not finding counter-examples". Certora's
  user docs open their
  [nonlinear-arithmetic section](https://docs.certora.com/en/latest/docs/user-guide/out-of-resources/timeout.html#dealing-with-nonlinear-arithmetic)
  with "Nonlinear integer arithmetic is often the hardest part of the formulas
  that the Certora Prover is solving", and name "modularization and
  underapproximation" as the main remedies, typically via method summaries.
  [Ghost functions with axioms](https://docs.certora.com/en/latest/docs/cvl/ghosts.html)
  are the user-level form of the same mechanism.
- **hevm** — [PR #1075, `--abstract-arith`](https://github.com/argotorg/hevm/pull/1075)
  (open as of 2026-09-29) makes `bvmul`/`bvudiv`/`bvurem` over symbolic operands
  uninterpreted, with "a catalogue of sound algebraic lemmas: commutativity, 0/1
  identities, mul/div/divisor monotonicity, the div–mul link `(a/b)*b ≤ a`,
  mulDiv bounds, …". On a satisfiable result it asserts real multiplication and
  re-checks: "A counterexample is only reported from that refined check."
- **Halmos** — makes symbolic multiplication and division uninterpreted
  functions by default (`f_evm_bvmul_256`, `f_evm_bvudiv_256` in
  [`sevm.py`](https://github.com/a16z/halmos/blob/main/src/halmos/sevm.py)), with
  only `(x / y) <= x`-style lemmas. Its
  [`counterexample-invalid` warning](https://github.com/a16z/halmos/wiki/warnings)
  documents the trade-off: "the nonlinear arithmetic reasoning is disabled by
  default, but this can sometimes result in generating invalid counterexamples."
- **Bitwuzla** does a solver-level version: Niemetz, Preiner, Zohar,
  [*Scalable Bit-Blasting with Abstractions*](https://bitwuzla.github.io/data/NiemetzPZ-CAV24.pdf),
  CAV 2024, on by default since 0.8.0 for terms of 33 bits and wider
  ([NEWS](https://github.com/bitwuzla/bitwuzla/blob/main/NEWS.md)). It cannot
  see through ruint, though: ruint compiles `U256 * U256` to 64-bit limb loops,
  so the solver never meets a 256-bit multiplication to abstract. Stubbing at
  the `Uint` level is what exposes one.
- **The underlying technique** is old: Bryant, Kroening, Ouaknine, Seshia,
  Strichman, Brady, [*Deciding Bit-Vector Arithmetic with
  Abstraction*](https://people.eecs.berkeley.edu/~sseshia/pubdir/uclid-tacas07.pdf),
  TACAS 2007.
- **Kani has no built-in form of it**: uninterpreted functions are an open
  request ([model-checking/kani#3112](https://github.com/model-checking/kani/issues/3112)),
  and [function contracts](https://model-checking.github.io/kani-verifier-blog/2024/01/29/function-contracts.html)
  "only work on crate-local items" — besides being unable to state two-call
  lemmas, as above. Stubs plus a memo table are the available route.

## Not done yet

In rough order of value:

1. **Automatic counterexample replay** on real arithmetic, reporting "real bug"
   or "missing lemma". Today it is manual.
2. **More lemmas.** hevm's list above is the obvious source: `(a / b) * b <= a`,
   distributivity, constant scaling.
3. **`%`, `checked_mul`, `overflowing_mul`** are still bit-precise under the
   oracle. `checked_mul` needs its own stub, since it bypasses `wrapping_mul`.
4. **One attribute instead of two stub lines per harness.** That is the kind of
   ergonomic need [00-project.md](00-project.md) says would justify a macro
   crate.
5. **Other widths.** Easy to generalise; left out until a contract needs it.
6. **Retire the `vested_experiment_*` harnesses**, which record how far exact
   arithmetic gets on monotonicity. `vested_is_monotone_in_time` now proves the
   general case.
7. **Storage.** The same principle — model the abstraction, not the bytes —
   applies to storage cells, but nothing here depends on it.
