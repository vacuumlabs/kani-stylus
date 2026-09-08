# kani-stylus — agent guide

Bounded model checking for Arbitrum Stylus contracts using Kani. See
[README.md](README.md) for setup.

## Read first

The knowledge base in [kb/](kb/) is the project's shared context. Start with
[kb/README.md](kb/README.md), which indexes the rest. At minimum, before doing
architectural work, read [kb/20-stylus.md](kb/20-stylus.md) (how the host
interface is actually structured) and [kb/50-feasibility.md](kb/50-feasibility.md)
(where the proposal is out of date, and the open risks).

[proposal.md](proposal.md) is a pitch, not a spec. Its central technical premise
is stale for stylus-sdk ≥ 0.10 — check it against the KB before implementing
from it.

## Conventions

- **Verify before asserting.** The SDK sources are vendored at
  `~/.cargo/registry/src/index.crates.io-*/stylus-{core,sdk,proc}-0.10.9/`.
  Read them rather than recalling API shapes; they change between versions.
- **Don't copy upstream docs into the repo.** Link to them from
  [kb/10-links.md](kb/10-links.md) and record only our own findings.
- **Keep [kb/50-feasibility.md](kb/50-feasibility.md) current.** When an
  experiment answers an open question, replace the question with the answer and
  say how it was verified.
- **Date and version-stamp KB facts** ("verified against stylus-sdk 0.10.9,
  2026-09-08") so staleness is visible.
- Kani runs take minutes. Use `cargo kani --harness <name>` while iterating, and
  run long verifications in the background.

## Verification rules learned the hard way

These are measured, not guessed — see [kb/50-feasibility.md](kb/50-feasibility.md).

- **Never let `std::HashMap`/`HashSet` be reachable from a proof harness.**
  `RandomState::new()` seeds SipHash via a `getrandom` syscall, which Kani
  cannot model; the whole harness aborts. This is why `TestVM` is unusable and
  why `SymbolicVM` uses fixed-size arrays. Use arrays, `BTreeMap`, or a
  deterministic hasher.
- **`U256` arithmetic does not trap.** `alloy`/`ruint` define `+`/`-` as
  `wrapping_add`/`wrapping_sub`, and Kani's automatic overflow checks only cover
  primitive integers. Overflow must be asserted explicitly — never assume a
  passing proof rules it out.
- **Locate cost cliffs with staged harnesses**, adding one layer at a time.
  Reading dependency trees to guess at the bottleneck produced two wrong
  diagnoses before a five-harness ladder found the real one in minutes.
- Prefer `--output-format terse`; the default buries results in warnings.
