# Local toolchain

Recorded 2026-09-08 on Linux 6.8.0 x86_64.

| Tool | Version | Notes |
| --- | --- | --- |
| `rustc` / `cargo` | 1.91.0 (f8297e351, 2025-10-28) | pinned by `stylus-samples/counter/rust-toolchain.toml` |
| `cargo kani` | 0.67.0 | `kani --version` agrees; toolchain under `~/.kani/kani-0.67.0` |
| `cargo stylus` | 0.10.9 | matches the `stylus-sdk` version in the counter |
| `docker` | 27.3.1 | needed by `cargo stylus` reproducible builds |
| `cast` (Foundry) | 1.6.0-nightly | chain interaction |
| `cbmc` | not on `PATH` | expected — Kani ships its own copy under `~/.kani` |

Targets: `wasm32-unknown-unknown` is installed for 1.91.0.

## Gotchas

**`rust-toolchain.toml` vs. Kani — resolved, not a problem.** Kani drives its own
bundled nightly, and it was suspected that a crate pinning a toolchain (as
`counter` pins 1.91.0 for the wasm target) would fight `cargo kani`. Tested
2026-09-08: `cargo kani --features proofs` runs fine inside
`stylus-samples/counter` with its pin in place. Don't restructure a project to
avoid this.

**Kani cannot target wasm.** Verification runs natively (x86_64), against
`--features stylus-test`. We are proving properties of the *Rust source*, not of
the deployed WASM bytecode. This is a real, stateable limitation: it does not
cover miscompilation or ArbOS-level semantics. Say so in the writeup rather than
letting a reviewer find it.

**Crate sources for reading.** `cargo fetch` in `stylus-samples/counter`
vendors them to
`~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/stylus-{core,sdk,proc,tools}-0.10.9/`.
`stylus-test` is an optional dependency and is only vendored once something
enables the `stylus-test` feature.

**Kani runs are slow.** A first run compiles the whole dependency tree through
the Kani compiler, then runs `goto-instrument` and the solver. Budget minutes,
not seconds, and prefer `--harness <name>` while iterating.

## Useful commands

```bash
# Stylus
cargo stylus new <name>
cargo build --target wasm32-unknown-unknown --release
cargo stylus check                      # will it activate on-chain?
cargo stylus export-abi
cargo test                              # TestVM-based unit tests

# Kani
cargo kani
cargo kani --harness <name> --output-format terse
cargo kani --features stylus-test
cargo kani --concrete-playback=print    # counterexample -> runnable #[test]
```
