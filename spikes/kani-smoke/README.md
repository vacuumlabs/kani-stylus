# kani-smoke

The feasibility spike described in [`../../kb/50-feasibility.md`](../../kb/50-feasibility.md).

Smallest possible question: can `cargo kani` verify anything at all about a
Stylus contract?

```bash
cargo kani --harness set_then_get_roundtrips
```

As of 2026-09-08 this **compiles and instruments cleanly but does not converge**
— killed at a 25-minute timeout during CBMC symbolic execution. The log is
dominated by `aho-corasick` / SIMD `memchr`, pulled in by the `lazy_static`
`Regex`es in `stylus-core/src/sol.rs`.

Next step is to stub those paths out and re-run. Keep this crate as the
convergence benchmark: if it turns green in a reasonable time, the project works.
