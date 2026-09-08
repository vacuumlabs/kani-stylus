# How Stylus works (the parts verification touches)

Verified by reading `stylus-sdk 0.10.9` / `stylus-core 0.10.9` source on
2026-09-08. Sources live in
`~/.cargo/registry/src/index.crates.io-*/stylus-{core,sdk,proc}-0.10.9/`.

## Execution model

A Stylus contract is Rust compiled to `wasm32-unknown-unknown` and executed by
ArbOS alongside the EVM. It shares the EVM's account model and its 256-bit
storage trie, so a Stylus contract and a Solidity contract are ABI-equivalent
and can call each other.

Contracts are `no_std` + `extern crate alloc`, and release profiles set
`panic = "abort"`. A Rust `panic!` therefore aborts the contract, burns the
remaining gas, and returns an opaque error rather than an ABI-encoded revert —
which is why "panic freedom" is a property worth proving mechanically.

## The host interface — three layers

This is the crux of the project, so it's worth being precise.

### Layer 1: raw ArbOS host calls (`stylus_sdk::hostio`)

`stylus-sdk/src/hostio.rs` declares the ArbOS surface through a `vm_hooks!`
macro. The macro expands **differently per cfg**:

```rust
cfg_if! {
    if #[cfg(feature = "export-abi")] {
        // each fn becomes `unimplemented!()`
    } else if #[cfg(feature = "stylus-test")] {
        // each fn becomes panic!("HostIO functions are not available in
        //                         stylus-test. Use TestVM functions instead.")
    } else {
        #[link(wasm_import_module = "vm_hooks")]
        extern "C" { /* the real imports */ }
    }
}
```

So the unresolved-`extern "C"` problem only exists on the default cfg. Under
`--features stylus-test` there are no extern symbols at all — just functions
that panic if anything reaches them. Reaching one during verification means a
code path bypassed the `Host` abstraction, and the panic makes that loud.

### Layer 2: the `Host` trait (`stylus_core::host`)

`stylus-core/src/host.rs` defines a composed trait:

```rust
pub trait Host:
    CryptographyAccess + CalldataAccess + UnsafeDeploymentAccess + StorageAccess
    + UnsafeCallAccess + BlockAccess + ChainAccess + AccountAccess + MemoryAccess
    + MessageAccess + MeteringAccess + RawLogAccess + DynClone {}
```

The sub-traits and their methods:

| Trait | Methods |
| --- | --- |
| `CryptographyAccess` | `native_keccak256` |
| `CalldataAccess` | `read_args`, `read_return_data`, `return_data_size`, `write_result` |
| `StorageAccess` | `storage_load_bytes32`, `storage_cache_bytes32` (unsafe), `flush_cache` |
| `MessageAccess` | `msg_sender`, `msg_value`, `msg_reentrant`, `tx_origin` |
| `BlockAccess` | `block_basefee`, `block_coinbase`, `block_number`, `block_timestamp`, `block_gas_limit` |
| `ChainAccess` | `chain_id` |
| `AccountAccess` | `balance`, `contract_address`, `code`, `code_size`, `code_hash` |
| `MeteringAccess` | `evm_gas_left`, `evm_ink_left`, `tx_gas_price`, `tx_ink_price`, `ink_to_gas`, `gas_to_ink` |
| `RawLogAccess` / `LogAccess` | `raw_log`, `log<T: SolEvent>` |
| `UnsafeCallAccess` | `call_contract`, `static_call_contract`, `delegate_call_contract` |
| `UnsafeDeploymentAccess` | `create1`, `create2` |
| `MemoryAccess` | `pay_for_memory_grow` |

Every one of the five host operations the proposal wants to stub
(`read_args`, `write_result`, `storage_load_bytes32`, `storage_cache_bytes32`,
`msg_sender`) is a **safe Rust trait method** here. Implementing them
symbolically is ordinary trait implementation, not FFI interception.

### Layer 3: `VM` and injection (`stylus_sdk::host`)

`stylus-sdk/src/host/mod.rs` defines a `VM` struct whose inner host is
cfg-selected:

```rust
#[cfg(not(feature = "stylus-test"))] pub struct VM { pub host: WasmVM }          // calls hostio
#[cfg(feature     = "stylus-test")]  pub struct VM { pub host: Box<dyn Host> }   // dynamic dispatch
```

Contracts reach it via `stylus_core::HostAccess`:

```rust
pub trait HostAccess { type Host; fn vm(&self) -> &Self::Host; }
```

The doc comment on `Host` states the intent outright:

> The host trait may be implemented by test frameworks as an easier way of
> mocking hostio invocations for testing Stylus contracts.

**This is the seam kani-stylus should use.** Under `--features stylus-test`,
`VM` holds a `Box<dyn Host>` and a contract is constructed as
`Counter::from(&vm)` — so any type implementing `Host` can be injected. A
`SymbolicVM` that returns `kani::any()` from these methods drops straight in
where `TestVM` goes.

Crucially, the generated `From` impl is **generic over any host**
(`stylus-proc-0.10.9/src/macros/storage.rs`):

```rust
impl<H: stylus_sdk::stylus_core::Host + Clone + 'static> From<&H> for Counter {
    fn from(host: &H) -> Self { /* ... Box::new(host.clone()) ... */ }
}
```

So `TestVM` is not privileged — it is just the implementation that ships. Our own
`SymbolicVM` drops in unchanged. The `stylus-test` *feature* is still needed
(it is what makes this impl exist and makes `VM` hold a `Box<dyn Host>`), but the
`stylus-test` *crate*'s code need never be reachable.

Notes measured on 2026-09-08 — see [50-feasibility.md](50-feasibility.md):

- Enabling `stylus-test` pulls in `alloy-provider` and friends — **268 crates**,
  including tokio, reqwest and hyper. This costs *compile* time only; none of it
  enters the goto program unless something constructs a `TestVM`. An upstream
  `mock-host` feature split would be a welcome cleanup but is not a blocker.
- `Box<dyn Host>` means dynamic dispatch on every host call. Measured cost so far
  is acceptable (a symbolic set/get proof runs in 14s), so this has not needed
  attention.

## Storage model

Stylus storage is the EVM's: `U256` slot key → `B256` 32-byte value, unset slots
read as zero. Reads are cached; writes land in a cache that `flush_cache`
persists. Contract fields are declared with `sol_storage!` / `#[storage]` and
slots are assigned Solidity-compatibly. Mappings hash key ‖ slot with keccak256.

Two consequences for a symbolic model:

- **Keccak is the hard part.** A faithful symbolic keccak256 is hopeless for an
  SMT solver. The standard move is to model it as an *uninterpreted injective
  function* — distinct preimages give distinct slots, no bit-level definition.
  This keeps mapping accesses cheap and non-aliasing. It's an assumption, and it
  should be stated in the proof output.
- **The slot map must be bounded.** An unbounded symbolic array is expensive.
  Start with a small association list (say 8–16 slots) of
  `(key, value)` pairs, unwritten keys reading as a fresh `kani::any()` or zero.

## Packaging: how verification attaches to a real contract

Verified 2026-09-08 against `examples/counter`, an unmodified
`cargo stylus new` project.

Two constraints interact, and getting either wrong is bad:

**1. `cargo kani` builds the `lib` target, so dev-dependencies are invisible.**
Putting `kani-stylus-core` in `[dev-dependencies]` fails with
`unresolved import kani_stylus_core`. It must be a regular dependency.

**2. `stylus-sdk/stylus-test` must never be on in a deployable build.** That
feature swaps the real ArbOS host for a mockable one — `hostio.rs` expands every
host call to `panic!("HostIO functions are not available in stylus-test")`. A
contract compiled with it enabled is broken on chain. But verification *needs*
it, because it is what makes the generic `From<&H>` impl exist and makes `VM`
hold a `Box<dyn Host>`.

The resolution is an opt-in feature that turns on both at once:

```toml
[dependencies]
kani-stylus-core = { path = "...", optional = true }

[dev-dependencies]                      # for `cargo test`, per the Stylus docs
stylus-sdk = { version = "0.10.9", features = ["stylus-test"] }

[features]
proofs = ["dep:kani-stylus-core", "stylus-sdk/stylus-test"]
```

with `cargo kani --features proofs`, and a guard so the feature can't be
forgotten:

```rust
#[cfg(all(kani, not(feature = "proofs")))]
compile_error!("proofs need the `proofs` feature: cargo kani --features proofs");
```

**Confirmed non-invasive.** On that project: `cargo test` passes, the release
wasm is a 18.5 KB cdylib containing no `kani` symbols and no `stylus-test` panic
stub (checked with `strings`) while still importing the real `vm_hooks`, and
`cargo stylus check` compiles and sizes it at 6.0 KB. Note that check's
*activation* step needs a Stylus RPC (default `localhost:8547`) — with a
devnode up it reported a 0.000071 ETH data fee; offline it stops after the size
report, which is a missing node rather than a problem with the contract.

**The `rust-toolchain.toml` pin is a non-issue.** The template pins Rust 1.91.0
for the wasm target; `cargo kani` drives its own toolchain regardless and was
unaffected. An earlier note in this KB treated this as a risk — it isn't.

## Mappings hash outside the `Host` trait

A trap worth knowing, verified 2026-09-08. `stylus-sdk/src/storage/map.rs`
computes mapping slots with:

```rust
crypto::keccak(data).into()
```

and `stylus-sdk/src/crypto.rs` is simply:

```rust
pub fn keccak<T: AsRef<[u8]>>(bytes: T) -> B256 {
    alloy_primitives::keccak256(bytes)
}
```

So mapping slot derivation **bypasses `CryptographyAccess::native_keccak256`
entirely**. Implementing that trait method is not enough to control hashing —
a host-level oracle will simply never be called, and real keccak256 reaches the
solver, which it cannot survive.

The way in is `#[kani::stub(stylus_sdk::crypto::keccak, ...)]` plus
`cargo kani -Z stubbing`. Kani's stubbing supports generic functions provided
arity and generic-parameter count match, which `keccak<T: AsRef<[u8]>>` does.
See [`crates/kani-stylus-core/src/keccak.rs`](../crates/kani-stylus-core/src/keccak.rs).

## Testing today: `TestVM`

`stylus-sdk` with `--features stylus-test` pulls in the `stylus-test` crate
(non-wasm targets only) providing `stylus_sdk::testing::TestVM` — a concrete
in-memory host with setters like `vm.set_value(...)`, `vm.set_sender(...)`.
`examples/counter/src/lib.rs` already uses it.

`TestVM` is the closest prior art and a useful reference for *what* each host
method should return — but it **cannot be used under Kani**. Its `VMState` holds
nine `std::HashMap`s, and `HashMap::new()` seeds SipHash via a `getrandom`
syscall, which Kani cannot model; verification aborts outright. See
[50-feasibility.md](50-feasibility.md) for the measurement.

So `SymbolicVM` differs from `TestVM` on two axes, not one: it returns
`kani::any()` instead of fixed values, *and* it must avoid `std` hash collections
entirely (fixed-size arrays, `BTreeMap`, or a deterministic hasher).
