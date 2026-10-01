# Canonical links

Every URL below returned HTTP 200 on 2026-09-08. Don't duplicate upstream
content into this repo — link here and record only our own findings.

## Arbitrum Stylus

Arbitrum publishes an LLM-oriented site index at
**<https://docs.arbitrum.io/llms.txt>** — a one-line description per page with
its canonical path. It's the fastest way for an agent to find the right page;
append `.md` to any docs path to get raw Markdown instead of rendered HTML.

### Start here
- [A gentle introduction to Stylus](https://docs.arbitrum.io/stylus/gentle-introduction) — what Stylus is and why WASM alongside EVM
- [Quickstart: write a contract in Rust](https://docs.arbitrum.io/stylus/quickstart) — the path that produced `examples/counter`
- [Prerequisites and setup](https://docs.arbitrum.io/stylus/fundamentals/prerequisites)
- [Structure of a Stylus Rust project](https://docs.arbitrum.io/stylus/fundamentals/project-structure)

### The bits that matter for verification
- [Testing smart contracts with Stylus](https://docs.arbitrum.io/stylus/fundamentals/testing-contracts) — `TestVM`, the mock host we intend to replace with a symbolic one
- [Stylus Rust SDK overview](https://docs.arbitrum.io/stylus/reference/overview) — crate structure and feature flags
- [Stylus Rust SDK advanced features](https://docs.arbitrum.io/stylus/reference/rust-sdk-guide)
- [Stylus Rust SDK storage](https://docs.arbitrum.io/stylus/fundamentals/data-types/storage) — the 256-bit slot model we must model symbolically
- [Global variables and functions](https://docs.arbitrum.io/stylus/fundamentals/global-variables-and-functions) — `msg::sender`, `block::timestamp`, etc.
- [Hostio exports](https://docs.arbitrum.io/stylus/advanced/hostio-exports) — the raw `extern "C"` ArbOS surface
- [VM and execution differences](https://docs.arbitrum.io/stylus/concepts/vm-differences) — EVM vs. WASM semantics
- [Security best practices](https://docs.arbitrum.io/stylus/best-practices/security) — a source of properties worth proving
- [Rust to Solidity differences](https://docs.arbitrum.io/stylus/advanced/rust-to-solidity-differences)

### Tooling and operations
- [Using Stylus CLI](https://docs.arbitrum.io/stylus/cli-tools/overview)
- [cargo-stylus command reference](https://docs.arbitrum.io/stylus/cli-tools/commands-reference)
- [Check and deploy](https://docs.arbitrum.io/stylus/cli-tools/check-and-deploy)
- [Configuration reference](https://docs.arbitrum.io/stylus/reference/stylus-toml-reference) — `Stylus.toml`, `Cargo.toml`, `rust-toolchain.toml`
- [Activation](https://docs.arbitrum.io/stylus/concepts/activation)
- [Gas metering](https://docs.arbitrum.io/stylus/concepts/gas-metering) — gas vs. ink
- [Gas and ink costs](https://docs.arbitrum.io/stylus/reference/opcode-hostio-pricing)
- [Debugging Stylus transactions](https://docs.arbitrum.io/stylus/cli-tools/debugging-tx)
- [Common issues and solutions](https://docs.arbitrum.io/stylus/troubleshooting/common-issues)
- [OffchainLabs/nitro-devnode](https://github.com/OffchainLabs/nitro-devnode) — the local dev node `cargo stylus check` wants on `localhost:8547`; `./run-dev-node.sh`, checked 2026-09-10

### Source and examples
- [OffchainLabs/stylus-sdk-rs](https://github.com/OffchainLabs/stylus-sdk-rs) — SDK source; read `stylus-core/src/host.rs` first
- [OffchainLabs/awesome-stylus](https://github.com/OffchainLabs/awesome-stylus) — curated ecosystem list
- [OpenZeppelin/rust-contracts-stylus](https://github.com/OpenZeppelin/rust-contracts-stylus) — audited ERC-20/721; the natural MVP proof target
- [Stylus by example: ERC-20](https://docs.arbitrum.io/stylus-by-example/applications/erc20) — smaller, self-contained alternative target
- API docs: [stylus-sdk 0.10.9](https://docs.rs/stylus-sdk/0.10.9/stylus_sdk/) · [`stylus_core::Host`](https://docs.rs/stylus-core/0.10.9/stylus_core/host/trait.Host.html)

## Kani

- [Kani book](https://model-checking.github.io/kani/) — the main reference
- [Installation](https://model-checking.github.io/kani/install-guide.html)
- [Tutorial](https://model-checking.github.io/kani/kani-tutorial.html)
  → [First steps](https://model-checking.github.io/kani/tutorial-first-steps.html)
  · [Kinds of failure](https://model-checking.github.io/kani/tutorial-kinds-of-failure.html)
  · [Loop unwinding](https://model-checking.github.io/kani/tutorial-loop-unwinding.html)
  · [Nondeterministic variables](https://model-checking.github.io/kani/tutorial-nondeterministic-variables.html)
  · [Verifying real code](https://model-checking.github.io/kani/tutorial-real-code.html)
- [Usage / CLI](https://model-checking.github.io/kani/usage.html)
- [Verification results](https://model-checking.github.io/kani/verification-results.html) — how to read SUCCESS / FAILURE / UNDETERMINED
- [Attribute reference](https://model-checking.github.io/kani/reference/attributes.html) — `#[kani::proof]`, `unwind`, `solver`, `stub`
- [Rust feature support](https://model-checking.github.io/kani/rust-feature-support.html) — check here before assuming a construct verifies
- [Limitations](https://model-checking.github.io/kani/limitations.html)
- [Undefined behaviour](https://model-checking.github.io/kani/undefined-behaviour.html) — what Kani does *not* catch
- [Comparison with other tools](https://model-checking.github.io/kani/tool-comparison.html) — bounded model checking vs. fuzzing vs. proof assistants
- [Application: where Kani fits](https://model-checking.github.io/kani/application.html)
- [Crate docs (`kani` API)](https://model-checking.github.io/kani/crates/index.html) — `kani::any`, `assume`, `cover`, `Arbitrary`
- Experimental: [Function contracts](https://model-checking.github.io/kani/reference/experimental/contracts.html) · [Loop contracts](https://model-checking.github.io/kani/reference/experimental/loop-contracts.html) · [Concrete playback](https://model-checking.github.io/kani/reference/experimental/concrete-playback.html)
- [model-checking/kani](https://github.com/model-checking/kani) — source and issue tracker
- [kani-verifier on docs.rs](https://docs.rs/kani-verifier/)

## Nonlinear arithmetic in other verifiers

Checked for a 200 on 2026-09-29. Discussed in
[35-arithmetic-oracle.md](35-arithmetic-oracle.md).

- [Hozzová et al., *Overapproximation of Non-Linear Integer Arithmetic for Smart Contract Verification*, LPAR 2023](https://easychair.org/publications/paper/BlrQ) — Certora's uninterpreted `*`/`div` plus instantiated axioms
- [Certora: dealing with nonlinear arithmetic](https://docs.certora.com/en/latest/docs/user-guide/out-of-resources/timeout.html#dealing-with-nonlinear-arithmetic) · [CVL ghosts and axioms](https://docs.certora.com/en/latest/docs/cvl/ghosts.html)
- [hevm PR #1075, `--abstract-arith`](https://github.com/argotorg/hevm/pull/1075) — uninterpreted `bvmul`/`bvudiv` with a lemma catalogue, refined before reporting
- [Halmos `sevm.py`](https://github.com/a16z/halmos/blob/main/src/halmos/sevm.py) · [`counterexample-invalid` warning](https://github.com/a16z/halmos/wiki/warnings)
- [Niemetz, Preiner, Zohar, *Scalable Bit-Blasting with Abstractions*, CAV 2024](https://bitwuzla.github.io/data/NiemetzPZ-CAV24.pdf) — Bitwuzla's solver-level abstraction
- [Bryant et al., *Deciding Bit-Vector Arithmetic with Abstraction*, TACAS 2007](https://people.eecs.berkeley.edu/~sseshia/pubdir/uclid-tacas07.pdf)
- [model-checking/kani#3112](https://github.com/model-checking/kani/issues/3112) — uninterpreted functions in Kani, open

## Storage models in other verifiers

Checked for a 200 on 2026-09-30; code links are pinned to a commit. Discussed
in [36-storage-model.md](36-storage-model.md).

- Certora: [havoc at rule start](https://docs.certora.com/en/latest/docs/user-guide/glossary.html#term-havoc) · [storage and memory analysis](https://docs.certora.com/en/latest/docs/prover/techniques/index.html#analysis-of-evm-storage-and-evm-memory) · [`-enableStorageSplitting`](https://docs.certora.com/en/latest/docs/prover/cli/options.html#enablestoragesplitting) · [keccak model](https://docs.certora.com/en/latest/docs/prover/approx/hashing.html#modeling-the-keccak-function-bounded-case) · [`StorageSplitter.kt`, the per-contract fallback](https://github.com/Certora/CertoraProver/blob/63fdea80a35b36ebfe0cff5dff9009b90e91966c/src/main/kotlin/analysis/split/StorageSplitter.kt#L143-L151) · [Grossman et al., memory splitting, OOPSLA 2024](https://cnandi.com/docs/oopsla24-cr.pdf)
- hevm: [`decomposeStorage`](https://github.com/argotorg/hevm/blob/c39757a24425bbc9c56b2c40a5d45faf4bfe70f4/src/EVM/Expr.hs#L1134-L1212) · [per-query fallback](https://github.com/argotorg/hevm/blob/c39757a24425bbc9c56b2c40a5d45faf4bfe70f4/src/EVM/SMT.hs#L114-L129) · [PR #436](https://github.com/argotorg/hevm/pull/436) · [decomposition fixes, PR #1094](https://github.com/argotorg/hevm/pull/1094)
- Halmos: [`GenericStorage`'s injective encoding](https://github.com/a16z/halmos/blob/079bb4241d1b460baf986257d56ea86977d73451/src/halmos/sevm.py#L2026-L2029) · [`--storage-layout`](https://github.com/a16z/halmos/blob/079bb4241d1b460baf986257d56ea86977d73451/src/halmos/config.py#L344-L348) · [#208, mixed layouts](https://github.com/a16z/halmos/issues/208)
- Kontrol: [keccak lemmas](https://github.com/runtimeverification/kontrol/blob/75bb958ddcebf9f2f0ff5aaa5712831303065612/src/kontrol/kdist/keccak.md)
- [Solidity SMTChecker encoding](https://docs.soliditylang.org/en/latest/smtchecker.html#smt-encoding-and-types) · [Move Prover, TACAS 2022](https://arxiv.org/abs/2110.08362)
- [OtterSec stellar-verify](https://github.com/otter-sec/stellar-verify/blob/f4e7a2a2563ab46d70b9185e51d584022ec8beb3/stellar/soroban-env-common/src/storage.rs) — Kani on Soroban
- Stylus: [Certora's WebAssembly hosts](https://github.com/Certora/CertoraProver/blob/63fdea80a35b36ebfe0cff5dff9009b90e91966c/lib/GeneralUtils/src/main/kotlin/cli/Converter.kt#L370-L375) (Soroban and NEAR only) · [Skribe final report](https://forum.arbitrum.foundation/t/skribe-advanced-fuzzing-for-stylus-final-milestone-report/30984) (a fuzzer)
- Kani stubbing limits: [#1997](https://github.com/model-checking/kani/issues/1997) (generic trait functions) · [PR #4587](https://github.com/model-checking/kani/pull/4587) (trait impls, 0.68) · [#4588](https://github.com/model-checking/kani/issues/4588) (default methods, open)
