# Local toolchain

Recorded 2026-09-08 on Linux 6.8.0 x86_64.

| Tool | Version | Notes |
| --- | --- | --- |
| `rustc` / `cargo` | 1.91.0 (f8297e351, 2025-10-28) | pinned by `examples/counter/rust-toolchain.toml` |
| `cargo kani` | 0.67.0 | `kani --version` agrees; toolchain under `~/.kani/kani-0.67.0` |
| `cargo stylus` | 0.10.9 | matches the `stylus-sdk` version in the counter |
| `docker` | 27.3.1 | needed by `cargo stylus` reproducible builds |
| `cast` (Foundry) | 1.6.0-nightly | chain interaction |
| `cbmc` | not on `PATH` | expected — Kani ships its own copy under `~/.kani` |

Targets: `wasm32-unknown-unknown` is installed for 1.91.0.

## Gotchas

**Mapping proofs need gigabytes, and an OOM can take your editor with it.**
Measured 2026-09-09: a single `cbmc` process on `vault::transfer_conserves_total`
reached **6.4 GiB resident** (7.0 GiB virtual). Two such proofs launched
concurrently from the VSCode integrated terminal exhausted a 23 GiB machine
(with only 979 MiB of swap). The global OOM killer shot one `cbmc` — and because
terminal children inherit the editor's systemd scope
(`app-code-*.scope`), systemd tore down **the whole scope**, killing VSCode:

```
Out of memory: Killed process 140836 (cbmc) total-vm:7053452kB, anon-rss:6710888kB
task_memcg=/user.slice/.../app.slice/app-code-5062.scope, task=cbmc
app-code-5062.scope: Failed with result 'oom-kill'
```

Two consequences:

- **Memory, not time, is the binding constraint on mapping proofs.** They are
  effectively serial on a laptop regardless of core count. The timings in
  [50-feasibility.md](50-feasibility.md) were all measured one-at-a-time; do not
  assume they parallelise.
- **Run long proofs in their own systemd scope**, so a cgroup OOM kills only the
  proof:

  ```bash
  systemd-run --user --wait --collect --unit=kani-<harness> \
      --property=MemoryMax=10G --property=MemorySwapMax=0 \
      --working-directory="$PWD" \
      -- cargo kani --features proofs -Z stubbing --harness <harness>
  ```

  A transient unit is a *sibling* of the editor's scope, not a child. Add
  `/usr/bin/time -v` around it to capture peak RSS.

**`rust-toolchain.toml` vs. Kani — resolved, not a problem.** Kani drives its own
bundled nightly, and it was suspected that a crate pinning a toolchain (as
`counter` pins 1.91.0 for the wasm target) would fight `cargo kani`. Tested
2026-09-08: `cargo kani --features proofs` runs fine inside
`examples/counter` with its pin in place. Don't restructure a project to
avoid this.

**Kani cannot target wasm.** Verification runs natively (x86_64), against
`--features stylus-test`. We are proving properties of the *Rust source*, not of
the deployed WASM bytecode. This is a real, stateable limitation: it does not
cover miscompilation or ArbOS-level semantics. Say so in the writeup rather than
letting a reviewer find it.

**Crate sources for reading.** `cargo fetch` in `examples/counter`
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
