# kani-stylus: formal verification for Arbitrum Stylus

**Team:** [Vacuumlabs](https://vacuumlabs.com) ·
**Event:** Arbitrum Open House Singapore 2026, online Buildathon ·
**Code:** <https://github.com/vacuumlabs/kani-stylus> (MIT)

Facts below were verified against `stylus-sdk` 0.10.9 and Kani 0.67.0. Every
measured number links to where it was recorded.

## 1. Summary

Stylus lets teams write Arbitrum contracts in Rust. The tools serious protocols
use to *prove* a contract correct before it holds money (Certora, Halmos, hevm,
Kontrol) are built for Solidity and EVM bytecode. We looked for one that
verifies Stylus and [found none](kb/36-storage-model.md#prior-art).

kani-stylus connects Stylus to [Kani](https://model-checking.github.io/kani/),
AWS's open-source model checker for Rust. You write a property once, and Kani
checks it for every input: every caller, every amount, every starting state.
When it fails, you get the exact input that breaks it, as a Rust test you can
paste in.

## 2. The problem

**`U256` arithmetic wraps silently.** The contract every Stylus developer
starts from, the `cargo stylus new` template, has a bug: `add_number` can make
the counter *smaller*. Its own unit test passes. `alloy`'s `U256 + U256` is
`wrapping_add`, so where Solidity would revert, Stylus silently succeeds. Rust
developers expect overflow to panic, and Kani's built-in overflow checks only
cover primitive integers, so the property has to be stated. The harness is
`add_number_can_decrease_the_counter` in
[`examples/counter`](examples/counter/src/lib.rs).

**Tests and fuzzing only check the inputs someone tried.** In the same
template, `increment` breaks at exactly one value in 2²⁵⁶. A fuzzer has to
guess it, but a solver derives it (`increment_wraps_at_max`).

**The SDK's own mock host can't be verified.** `TestVM` holds `std::HashMap`s,
which seed SipHash with a `getrandom` syscall that Kani cannot model, so
verification aborts before it reaches any contract code
([kb/50-feasibility.md](kb/50-feasibility.md)). Handing a Stylus contract to
Kani does not work out of the box.

## 3. What we built

Since `stylus-sdk` 0.10, contracts reach ArbOS through a safe Rust trait,
`stylus_core::Host`, and the generated code is generic over it
([kb/20-stylus.md](kb/20-stylus.md)). So the work is not intercepting FFI.
It is a *symbolic* host, plus the models that make proofs over it finish in
minutes.

| Component | In [`crates/kani-stylus-core`](crates/kani-stylus-core/) | What it does |
| --- | --- | --- |
| Symbolic host | `host.rs`, `context.rs` | `SymbolicVM` implements `stylus_core::Host`. By default the caller, value, origin, block number, timestamp and chain id are all symbolic. |
| Storage model | `storage.rs`, `slots.rs` | Mapping slots are structured by default. The `precise-storage` feature runs the SDK's own keccak derivation instead. `with_arbitrary_storage()` starts a proof from any state. See [kb/36](kb/36-storage-model.md). |
| Keccak oracle | `keccak.rs` | keccak256 as an uninterpreted, injective function. |
| Arithmetic oracle | `arith.rs`, `arith_oracle.rs` | Exact `U256` division, and lemma-constrained `*` and `/` for `x * y / z` business logic. See [kb/35](kb/35-arithmetic-oracle.md). |
| Frame conditions | `snapshot()`, `slots_changed_since()` | For "touches nothing else" properties. |
| Runner | [`verify.sh`](verify.sh) | Runs every suite. `--playback` prints a counterexample. |

It drops into the project `cargo stylus new` gives you: one optional
dependency and a `proofs` feature. `cargo test`, `cargo build` and
`cargo stylus check` don't change, and the built wasm contains no proof code
([README](README.md#it-goes-in-your-normal-stylus-project)).

## 4. What it proves today

There are three examples, each a real `cargo stylus new` project with proofs
added in place. All 34 harnesses run by default end as they should, and on a
laptop `./verify.sh vault` runs all 14 of the vault's in under 11 minutes
([measured](kb/36-storage-model.md#measured) 2026-09-30 and 2026-10-01).

- **[`counter`](examples/counter/)**, 7 harnesses: the template's `add_number`,
  `mul_number` and `increment` all wrap, each found with an exact witness.
- **[`vault`](examples/vault/)**, 14 harnesses, on an ERC-20-shaped contract:
  - only the owner can transfer ownership, for every caller;
  - `transfer` and `transferFrom` conserve total supply, from an arbitrary state;
  - `transferFrom` spends exactly the allowance and moves no other balance or
    allowance;
  - distinct accounts never alias.
- **[`vesting`](examples/vesting/)**, 13 harnesses by default: the vested amount
  never decreases over time, for every schedule and any total up to 2¹⁹². Proofs
  also find a schedule overflow and a re-initialisation bypass through the zero
  address.

## 5. Limits, stated plainly

- **One call from any state.** Each proof covers a single method call.
  Conservation over *sequences* of calls rests on a hand induction over the
  per-call lemmas, and is written down as such.
- **Method level, not calldata level.** Proofs call methods directly. The ABI
  router generated by `#[public]` is not verified yet.
- **Rust source, not wasm.** Kani checks the source. It says nothing about
  miscompilation or ArbOS semantics.
- **Models are assumptions.** Structured slots and the arithmetic oracle trade
  bit-level precision for proofs that finish. Each assumption is listed in
  [kb/36](kb/36-storage-model.md#assumptions-all-together) and
  [kb/35](kb/35-arithmetic-oracle.md). The `precise-storage` feature swaps in
  the SDK's real slot derivation where a proof needs it.
- **No cross-contract calls.** `call_contract` and friends are
  `unimplemented!()`, so a proof that reaches one fails loudly instead of
  passing.

## 6. Roadmap

In order. The reasoning and the measurements behind it are in
[kb/60-roadmap.md](kb/60-roadmap.md).

1. **Properties over sequences of calls.** A bounded dispatcher over symbolic
   actions, and inductive invariants from an arbitrary state.
2. **A property library.** Conservation, access control, monotonicity,
   no-aliasing and panic freedom, instantiated by naming a contract's methods
   instead of writing proofs.
3. **OpenZeppelin's [Stylus contracts](https://github.com/OpenZeppelin/rust-contracts-stylus)**
   verified unmodified, or a precise account of why not.
4. **Modular verification** through Kani's function contracts, so a proved
   method's spec is reused at its call sites.
5. **Calldata-level proofs** through the `#[public]` router.
6. **Cross-contract calls and reentrancy.**
7. **A GitHub Action** that runs the proofs on every pull request.

Out of scope: full symbolic decoding of dynamic ABI types, and AST linting.

## 7. Why it matters for Arbitrum

Formal verification is how high-value EVM contracts are checked before they
ship. A protocol weighing Stylus has none today, which is a reason to keep its
critical code in Solidity. kani-stylus closes that gap with open-source tooling
that fits the Rust workflow Stylus developers already use. After the
Buildathon, we intend to seek Arbitrum ecosystem grant funding to deliver the
roadmap above.

---

*This replaces the 2026-09-08 draft, which assumed an older SDK in which the
host was reached through raw `extern "C"` calls. The draft is in git history
(`git show 9bcfbab:proposal.md`), and what it got wrong is recorded in
[kb/50-feasibility.md](kb/50-feasibility.md).*
