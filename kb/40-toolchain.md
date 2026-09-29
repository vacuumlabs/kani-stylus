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
      --property=MemoryMax=<cap> --property=MemoryHigh=<cap minus 1G> \
      --property=MemorySwapMax=0 \
      --nice=19 --property=CPUWeight=10 --property=IOWeight=10 \
      --working-directory="$PWD" \
      -- cargo kani --features proofs -Z stubbing --harness <harness>
  ```

  A transient unit is a *sibling* of the editor's scope, not a child. Add
  `/usr/bin/time -v` around it to capture peak RSS.

  **Size `<cap>` to what is free, not to a fixed number.** Measured
  2026-09-29: a full `./verify.sh vesting` inside `MemoryMax=10G` still made
  the 23 GiB laptop unusable, because the desktop already held ~10 GiB and
  `kani-driver` peaks at ~8 GiB on `vested_is_monotone_in_time`. The cap stops
  an OOM from spreading; it does nothing about machine-wide memory pressure.
  Take `available` from `free -m`, leave a few GiB of headroom, and let the job
  be OOM-killed inside its cgroup rather than squeeze everything else.
  `MemoryHigh` makes the kernel reclaim from the job before it reaches the cap,
  and the nice/weight settings keep the desktop responsive while it runs.
  Most of that ~8 GiB goes on assertion-reachability checks: for a measurement
  run where vacuity is checked another way, `--no-assertion-reach-checks` cuts
  `kani-driver` to tens of MiB (see [35-arithmetic-oracle.md](35-arithmetic-oracle.md)).

**`rust-toolchain.toml` vs. Kani — resolved, not a problem.** Kani drives its own
bundled nightly, and it was suspected that a crate pinning a toolchain (as
`counter` pins 1.91.0 for the wasm target) would fight `cargo kani`. Tested
2026-09-08: `cargo kani --features proofs` runs fine inside
`examples/counter` with its pin in place. Don't restructure a project to
avoid this.

**`cfg(kani)` warns under `cargo build`/`cargo test` unless declared.** The
proof modules sit behind `#[cfg(kani)]`, and that cfg is injected only by
`cargo kani`. To every other cargo invocation `kani` is an unknown cfg name, so
rustc's `unexpected_cfgs` lint (on by default since Rust 1.80) fires once per
site — 2 warnings in each example, 9 in `kani-stylus-core`. It is a lint about
the cfg *name*, not about the code: nothing is misconfigured and the proofs are
unaffected. Fixed 2026-09-10 by declaring the cfg in each package's
`Cargo.toml`:

```toml
[lints.rust]
unexpected_cfgs = { level = "warn", check-cfg = ['cfg(kani)'] }
```

Declare rather than `allow`: unknown *other* cfg names still warn. The examples
are excluded from the workspace, so they each need their own copy — a
`[workspace.lints]` entry would not reach them.

**The template's bin target collides with the cdylib, and can leave an empty
wasm at the path you deploy.** Verified 2026-09-10. `cargo stylus new` gives you
both a `[lib]` with `crate-type = ["lib", "cdylib"]` and a `src/main.rs` (there
only so `cargo stylus export-abi` has a `main` to run). Both are named after the
package, so `cargo build --target wasm32-unknown-unknown --release` warns twice
about an "output filename collision" over
`target/wasm32-unknown-unknown/release/<name>.wasm` — and the collision is not
cosmetic. The bin compiles to a **109-byte** module with no `user_entrypoint`;
whichever target is written last wins the path. We observed the stub sitting
there in place of the real 18.5 KB contract.

`cargo stylus check`/`deploy`/`verify` are immune on two counts: `stylus-tools`
builds with `cargo build --lib --locked --release` and reads
`.../release/deps/<name>.wasm`, where the cdylib keeps the plain name and the bin
gets a hash suffix. The hazard is anything reading the *top-level* path — a hand
written deploy step, a CI job, `cargo stylus check --wasm-file`.

Fixed by renaming the bin in each example's `Cargo.toml`:

```toml
[[bin]]
name = "<name>-abi"
path = "src/main.rs"
```

`cargo stylus export-abi` still works: it runs `cargo run --package <name>
--features export-abi` without naming a bin, and there is still exactly one.

**Do not reach for `required-features = ["export-abi"]` here**, the otherwise
idiomatic fix. It satisfies cargo but breaks verification: `cargo kani` builds
all targets and dies on a bin filtered out by features —
`error: target `counter` in package `counter` requires the features:
`export-abi``. Measured 2026-09-10.

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
