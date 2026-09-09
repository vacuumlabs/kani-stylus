# Playing with the solver directly

Verified 2026-09-09 against Kani 0.67.0 / CBMC 6.8.0 / kissat 4.0.1 / Z3 4.8.10.

## The stack

```
#[kani::proof]  ->  kani-compiler  ->  goto-IR  ->  formula  ->  SAT or SMT solver
```

Kani itself proves nothing. CBMC symbolically executes the goto program, turns
the program-plus-negated-assertions into a formula, and hands it to a solver.
`unsat` means no counterexample exists — the property holds within the bound.
`sat` means the solver found one, and CBMC decodes the model back into the
concrete values you see in the report.

Note that CBMC and Z3 are not alternatives: CBMC is not a solver, it is the
layer that *produces* what a solver consumes. Kani already uses Z3, optionally,
through CBMC. Two solver layers are reachable:

- **SAT** — everything bit-blasted to CNF. Kani's default is **CaDiCaL** (built
  into CBMC); **kissat** ships alongside it at `~/.kani/kani-0.67.0/bin/kissat`.
- **SMT** — CBMC emits SMT-LIB 2 in the `QF_BV` (quantifier-free bitvector)
  theory and shells out to Z3 or CVC5. Only Z3 is installed here.

Pick one per harness with `#[kani::solver(kissat)]` or `--solver`; the choices
are `bitwuzla, cadical, cvc5, kissat, minisat, z3, bin=<path>`.

## Who does what

**No C is generated from your Rust.** `kani-compiler` *is* rustc — a driver that
swaps the backend, walking rustc's MIR and emitting CBMC's goto-IR directly via
[`cprover_bindings`](https://github.com/model-checking/kani/tree/main/cprover_bindings).
The `.symtab.out` files under `target/kani/` begin with the magic bytes
`7f 47 42 46` = `GBF`, goto binary format. The only C in the pipeline is
`~/.kani/<ver>/library/kani/kani_lib.c` — 125 lines of `malloc`/`memcpy` shims
plus an assert-then-assume macro.

The real pipeline, from `cargo kani --verbose`:

```
cargo rustc  RUSTC=kani-compiler   ->  <harness>.symtab.out    (GBF)
goto-cc  <symtab.out>  kani_lib.c  -o  <harness>.out           link
goto-cc  <.out>  --function <mangled_harness>                  set entry point
goto-instrument --add-library --no-malloc-may-fail             CBMC's C model lib
goto-instrument --generate-function-body .* \
                --generate-function-body-options assert-false-assume-false \
                --drop-unused-functions                        stub bodyless fns, prune
goto-instrument --ensure-one-backedge-per-target               loop normalization
cbmc --sat-solver cadical --slice-formula --object-bits 16 ...
```

**Kani owns everything Rust-shaped**, which CBMC has no concept of:

- **MIR → goto-IR**: monomorphization, trait objects and vtables, enum niche
  layouts, fat pointers, `Box`. CBMC's type system is C's; someone must decide
  how `dyn Host` becomes it.
- **Its own precompiled `std`** — note `--sysroot ~/.kani/<ver>` and
  `--extern noprelude:std=.../libstd.rlib`, built with `-Z always-encode-mir`.
  Real `std` ships without MIR for most functions, so Kani cannot translate it.
  This is why Kani pins a specific nightly, and its largest maintenance cost.
- **Harness discovery and reachability** —
  `-Cllvm-args=--reachability=harnesses --harness <name>` (Kani smuggles its own
  options through the `llvm-args` channel). Each harness gets its own goto
  binary, which is why `--harness` is so much faster than a full run.
- **Attribute plumbing** — `-Z crate-attr=register_tool(kanitool)` is what makes
  `#[kani::proof]` a legal attribute; `--cfg=kani` is what makes `#[cfg(kani)]`
  work.
- **Rust semantics over C semantics** — it passes `-C overflow-checks=on` to
  rustc *and* `--no-signed-overflow-check` to CBMC, so the overflow check you
  get is Rust's panic branch, not C's. That is also exactly why the KB's U256
  rule holds: `alloy` calls `wrapping_add` explicitly, so no check is emitted.
- **The `kani::` API and result reporting** — mapping CBMC's JSON back to
  harness names and source lines.

So CBMC is the verification engine and Kani is the Rust frontend plus driver.
"Just use CBMC" would mean writing the contracts in C.

## Bit-blasting

Compiling a formula over fixed-width integers down to pure Boolean logic: one
variable per *bit*, arithmetic replaced by the circuit that implements it. `x + y`
on `u8` becomes an 8-stage ripple-carry adder in CNF. Nothing in the result knows
what a number is. CBMC announces the step as `Running propositional reduction`.

Measured here with `__CPROVER_assert(x * y == y * x)` at several widths:

| width | CNF variables | clauses |
| --- | --- | --- |
| 8-bit | 1,002 | 4,573 |
| 32-bit | 3,266 | 16,783 |
| 64-bit | 12,674 | 68,367 |
| 128-bit | 49,922 | 275,983 |

Doubling the width roughly quadruples the formula — multiplication is quadratic
in bit width, being a grid of partial products. Extrapolating, **one** 256-bit
multiply is on the order of 200k variables and 1.1M clauses. That is the whole
story of why `U256` is this project's cost driver, and why the KB advises
proving over `u64`-shaped values first.

Addition is linear by contrast: 64-bit → 321 variables, 128-bit → 641. Both came
out with **zero clauses**, because CBMC's simplifier normalized `x + y == y + x`
to `true` before the bit-blaster saw it. A simplification layer sits in front;
not everything you write reaches the solver.

The trade-off against SMT: bit-blasting discards all structure, so nothing can
reason about `x * y` algebraically — but it hands the problem to a CDCL SAT
solver, which is extraordinarily good at huge unstructured formulas. For this
workload that usually wins, hence `--sat-solver cadical` above.

## goto-IR

CBMC's intermediate representation: a **goto program**. Structured control flow
is gone — each function is a flat list of instructions (`ASSIGN`, `ASSUME`,
`ASSERT`, `GOTO`, `FUNCTION_CALL`, `DECL`) with conditional jumps, hence the
name. Types and expressions are C-shaped (`signedbv`, `unsignedbv`, `pointer`,
`array` — visible as plain strings in a symtab dump). Serialized as `GBF`
binary, or JSON.

- [`src/goto-programs/README.md`](https://github.com/diffblue/cbmc/blob/develop/src/goto-programs/README.md) — the prose explanation; best starting point
- [goto-programs module reference](https://diffblue.github.io/cbmc/group__goto-programs.html) — API level
- [Kani developer documentation](https://model-checking.github.io/kani/dev-documentation.html) — the MIR→goto side

To read one: `goto-instrument --show-goto-functions <harness>.out` prints it as
text (`--keep-temps` leaves the binary behind).

## Write formulas by hand

```bash
~/.kani/kani-0.67.0/bin/kissat -q sat.cnf   # -> s SATISFIABLE / v -1 2 -3 0
z3 u256.smt2                                # -> sat + model, then unsat
```

- [sat.cnf](sat.cnf) — DIMACS CNF, the SAT layer's only input format.
- [u256.smt2](u256.smt2) — SMT-LIB 2. `QF_BV` is the theory our proofs actually
  live in: a `U256` *is* `(_ BitVec 256)`, and alloy's `+` *is* `bvadd`, which
  is why [the KB says U256 arithmetic never traps](../../kb/50-feasibility.md).
  Shows both directions — ask for a witness (`sat`), and prove a claim by
  asserting its negation (`unsat`).

kissat exits **10** for sat, **20** for unsat, so it scripts cleanly.

For formulas generated from *real Rust*, small enough to read line by line,
see [../kani/](../kani/) — including what a `#[kani::stub]` does to a formula.

## Dump the formula Kani actually generates

`--cbmc-args` is behind `-Z unstable-options`, and `--outfile` makes CBMC write
instead of solve:

```bash
cd examples/counter
cargo kani -Z unstable-options --features proofs --harness starts_at_zero \
  --cbmc-args --dimacs --outfile /tmp/h.cnf     # CNF
cargo kani -Z unstable-options --features proofs --harness starts_at_zero \
  --cbmc-args --z3    --outfile /tmp/h.smt2     # SMT-LIB 2
```

Kani prints `VERIFICATION:- FAILED` afterwards — ignore it, that is just Kani
misreading an exit code from a run that dumped instead of solving.

Sobering scale check: `starts_at_zero` is the most trivial harness in the repo
(assert a fresh counter reads zero) and its CNF is **273,169 variables /
529,469 clauses**, 15 MB. The SMT2 form is 10 MB. Most of that is `SymbolicVM`
and the SDK, not the property.

Caveat: the `--dimacs` dump keeps the property literals as free variables
rather than asserting them, so feeding it to kissat returns `sat` regardless of
the verdict. Use the dumps to *measure* formula size and inspect encodings, not
to reproduce a result. `--keep-temps` leaves the goto binary next to it
(`target/kani/.../<mangled-harness>.out`) for `goto-instrument --show-goto-functions`.

## Docs

- [SMT-LIB 2 standard](https://smtlib.cs.uiowa.edu/language.shtml) · [theory of fixed-size bitvectors](https://smtlib.cs.uiowa.edu/theories-FixedSizeBitVectors.shtml)
- [Z3 guide](https://microsoft.github.io/z3guide/) — the interactive tutorial is the fastest way in
- [kissat](https://github.com/arminbiere/kissat) · [DIMACS CNF format](https://jix.github.io/varisat/manual/0.2.0/formats/dimacs.html)
- [CBMC manual](https://diffblue.github.io/cbmc/) · `~/.kani/kani-0.67.0/bin/cbmc --help`
- [Kani: `#[kani::solver]`](https://model-checking.github.io/kani/reference/attributes.html#kanisolver)
