# learning

Scratch space for getting (re)acquainted with the machinery. Unlike [kb/](../kb),
nothing here is load-bearing for the project — these are notes to think with.

- [solver/](solver/) — the layer under Kani: who does what in the
  Kani→CBMC→solver pipeline, what bit-blasting is and what it costs, goto-IR,
  and hand-written CNF/SMT2 to poke at.
- [kani/](kani/) — tiny pure-Rust harnesses whose formulas are short enough to
  read, each paired with a hand-written Z3 query. Ends on what
  `#[kani::stub]` does at the formula level.
