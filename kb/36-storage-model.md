# Storage: structured by default, precise on request

Verified against Kani 0.67.0, CBMC 6.8.0, stylus-sdk 0.10.9, 2026-09-30. The
code is [`storage.rs`](../crates/kani-stylus-core/src/storage.rs) (the store),
[`slots.rs`](../crates/kani-stylus-core/src/slots.rs) (mapping slots),
[`host.rs`](../crates/kani-stylus-core/src/host.rs) (the host) and the
`proof!` macro in [`lib.rs`](../crates/kani-stylus-core/src/lib.rs). The
worked example is [`examples/vault`](../examples/vault/src/lib.rs).

The companion to [35-arithmetic-oracle.md](35-arithmetic-oracle.md), and the
same principle: users verify business logic while *trusting* the layer below,
instead of re-proving it bit by bit in every harness.

## The problem

Storage, not business logic, was most of the formula. With no business logic
at all — write fields, read them back — the old model cost:

| Harness (no logic, `SymbolicVm<8>`) | Time | Clauses | Peak RSS |
| --- | --- | --- | --- |
| bind a contract, touch nothing | 4.9s | 0.49M | 0.4 GiB |
| four `U256` fields written and read back | 32.8s | 4.94M | 1.0 GiB |
| three packed fields (`address`, `bool`, `uint64`) | 45.7s | 4.45M | 1.0 GiB |
| two symbolic mapping keys | 66.3s | 4.30M | 1.3 GiB |
| three symbolic mapping keys | 130.2s | 6.96M | 2.3 GiB |

Every extra key doubled the cost. A contract with a few maps and a few calls
per harness was out of reach before its logic had been looked at.

All figures in this note are one run each, `--no-assertion-reach-checks`,
concrete context, on an otherwise idle 4-core laptop; see the method notes at
the end. The harnesses are a scratch crate, not committed; each is a few lines
over the contract below, and every one carries a `kani::cover!` that was
satisfied.

```rust
sol_storage! {
    pub struct Ladder {
        uint256 s0; uint256 s1; uint256 s2; uint256 s3;
        address owner; bool paused; uint64 stamp;          // packed into one slot
        mapping(address => uint256) bal;
        mapping(address => mapping(address => uint256)) allow;
    }
}
```

## Where the cost was: a ladder

Following the rule in [CLAUDE.md](../CLAUDE.md), one layer replaced per rung.
Three layers turned out to matter, and a fourth did not.

| Rung | Old | direct store | + zero-sized host | model-neutral (E4) | **structured slots (E5)** |
| --- | --- | --- | --- | --- | --- |
| bind | 4.9s / 0.49M | — | 2.1s / 0.08M | — | — |
| four `U256` fields | 32.8s / 4.94M | 13.3s / 1.14M | 7.0s / 0.33M | 8.0s / 0.33M | — |
| three packed fields | 45.7s / 4.45M | 12.0s / 1.07M | 6.5s / 0.22M | — | — |
| one symbolic key | 30.2s / 2.19M | — | — | 11.1s / 0.41M | — |
| two symbolic keys | 66.3s / 4.30M | — | — | 35.6s / 1.13M | **7.6s / 0.33M** |
| three symbolic keys | 130.2s / 6.96M | — | — | 82.8s / 2.35M | **16.3s / 0.59M** |
| nested map, one entry | 38.5s / 2.95M | — | — | 31.6s / 0.97M | **5.5s / 0.22M** |

Time / clauses. What each column changes:

1. **The flat slot list** (*direct store*). The old `SlotStore` put every slot,
   a contract's own fields included, into one association list: every access
   scanned it, and every write appended at a position that became symbolic as
   soon as any earlier access branched. Replacing it with a direct-indexed
   array — hooked in at `GlobalStorage::get_word`/`set_word` for the
   experiment — cut four fields from 4.9M clauses to 1.1M.
2. **The host's plumbing** (*zero-sized host*). The SDK clones its
   `Box<dyn Host>` on every storage access
   (`self.__stylus_host.clone()` in `stylus-sdk/src/storage/mod.rs`). The old
   host kept its state behind `Rc<RefCell<..>>`, so each clone allocated a box
   and bumped a count, and each access borrowed. With the state global, like
   the oracles', the host is zero-sized and `Box::new` of a zero-sized type
   allocates nothing: binding the contract went from 280k variables to 65k.
   Keeping a per-handle context in the host instead (non-zero-sized, no
   `Rc`) landed in between — 310k variables and 12.8s on four fields, against
   221k and 7.0s — so the context went global too.
3. **Keccak for mapping slots** (*structured slots*). With 1 and 2 fixed, the
   keccak oracle was the rest: fresh 256-bit digests, a preimage table, and a
   distinctness assumption per pair. Numbering `(map, key)` pairs instead,
   below, cut two keys from 1.13M clauses to 0.33M, and the gap widens with
   every key.
4. **The SDK's byte packing** did not matter much once 1–3 were gone: about
   20k variables per access remain, packing included. That is fortunate, since
   it is out of reach anyway (see [Kani limits](#kani-limits-that-shaped-this)).

The column *model-neutral (E4)* is 1 and 2 with the keccak oracle kept: same
semantics as the old model, but cheaper. The shipped implementation is E5 with
the store hardened for general use; its numbers are in
[Measured](#measured).

## What changed

Two of the changes are encodings, with no effect on what is proved. One is an
abstraction, and it is the default. One is new API.

### A zero-sized host (encoding)

`SymbolicVm<SLOTS>` is now a unit struct; storage and the transaction context
are statics, as the keccak and arithmetic oracles already were. Kani gives
every harness its own program, so each proof starts from empty state.

The cost is one chain state per proof. A second `SymbolicVm::new()` fails the
proof; handles made by cloning or by `with_timestamp` share storage, and
changing the context changes it for every handle — which is the next
transaction, as on chain. No existing harness built two VMs.

### A two-tier store (encoding)

Slots below `SMALL_SLOTS` (32) — a contract's own fields — are direct-indexed;
every other slot sits in an association list, scanned limb by limb. Values are
held as four `u64`s rather than `B256`'s 32 bytes, because symbolic execution
tracks each array element separately. Three details were each measured by
changing only that detail:

- **Read and write inside the scan.** Returning the index and using it after
  the loop indexes the arrays symbolically: on two structured keys, 12.1s and
  0.51M clauses against 10.1s and 0.36M. About even under `precise-storage`
  (36.9s against 36.2s).
- **`SMALL_SLOTS` is not free.** A write to a slot symbolic execution cannot
  prove large — a keccak digest, in the precise model — muxes over every
  small cell: on two keys under `precise-storage`, 16, 32 and 64 cells took
  32.5s, 36.2s and 39.4s (1.39M, 1.63M, 2.10M clauses). Structured slots are
  provably large, so they don't pay. 32 is the compromise; a field beyond it
  still works, from the list.
- **`MAX_SLOTS`, the list's capacity, adds nothing to the formula** — the
  scans stop at the harness's `SLOTS` — but symbolic execution still tracks
  it: on three keys, 64 entries took 17.1s of symex against 11.0s for 16,
  with an identical formula. It is 32.

**Checked, not argued.** hevm fixed two bugs in its own storage decomposition
this month — dropped array writes, and a small-slot read returning 0 (see
[Prior art](#prior-art)) — so the store carries Kani proofs of its own, in
`storage::proofs`:

- `two_tier_store_refines_a_flat_map` — any sequence of four loads and stores
  on slots drawn from both tiers and from arbitrary 256-bit values returns
  exactly what a flat `==`-compared association list returns;
- `arbitrary_storage_is_consistent` and `changed_since_counts_what_moved` —
  the two features below.

`cargo kani -p kani-stylus-core`; all four verify, with their covers
satisfied (2026-09-30, default flags):

| Proof | Checks | Time |
| --- | --- | --- |
| `two_tier_store_refines_a_flat_map` | 0 of 369 failed | 406s |
| `arbitrary_storage_is_consistent` | 0 of 488 failed | 213s |
| `changed_since_counts_what_moved` | 0 of 522 failed | 119s |
| `slots::proofs::structured_slots_are_injective_and_disjoint` | 0 of 325 failed | 16s |

The refinement proof stores values with one symbolic byte: the store only
moves values, and full 32-byte values ran past 19 minutes.

### Structured mapping slots (the abstraction)

A mapping entry lives at `keccak256(pad32(key) ‖ root)`, `root` being the
map's own slot. No contract ever sees that slot; only the SDK's storage types
do, and they use it for two things only: as a storage key, and as a base for
small offsets (a struct value's fields). So storage correctness never depends
on the digest's *value*. It depends on distinct `(root, key)` pairs getting
disjoint regions of slot space — the collision freedom every Solidity and
Stylus layout already rests on.

[`slots`](../crates/kani-stylus-core/src/slots.rs) states that directly.
Stubs for `StorageKey::to_slot` number pairs in order of first use, and give
pair `i` the slot `TAG·2^192 + i·2^128`. Distinct pairs get distinct numbers;
the stride leaves any offset the SDK adds inside its region; and the keccak
oracle assumes its digests avoid the `TAG` space, 2^-64 of all slots, so the
two kinds never alias. Nothing is hashed, and slots are compared as numbers
whose bits are almost all constant.
`slots::proofs::structured_slots_are_injective_and_disjoint` checks both
properties for arbitrary roots, keys and offsets below 2^128.

**Why a counterexample stays real.** Unlike
[`arith_oracle`](35-arithmetic-oracle.md), which knows only some facts about
`*` and `/`, this is not an over-approximation. It is a *different injective
layout*, and a contract behaves identically under every injective,
region-disjoint layout, because the only thing it can do with a slot is use
it. So `#[kani::should_panic]` harnesses may use it.

**Why only for storage.** User code calling `stylus_sdk::crypto::keccak`
still gets the keccak oracle's fresh digests. A structured slot is a very
particular number, and a contract that inspects a hash — orders two, or
reduces one mod `n` — must not be verified against it.

**Which keys.** `Address`, `bool` and the unsigned integers. `U256`,
`FixedBytes<N>` (so `B256`) and `Signed` keys go through *generic* impls,
which Kani 0.67 cannot stub; byte-string keys, and the element slots of
`StorageVec` and `StorageBytes`, hash the base directly. All of those keep the
keccak oracle, in either mode — correct, just at the old per-key price.

### The fidelity flag

Harnesses that touch mappings are declared with `proof!`, which turns each
function into a `#[kani::proof]` and attaches the stubs:

```rust
kani_stylus_core::proof! {
    fn transfer_conserves_total() { /* ... */ }

    #[kani::should_panic]
    fn credit_can_silently_wrap() { /* ... */ }
}
```

```bash
cargo kani --features proofs -Z stubbing                                  # structured
cargo kani --features proofs,kani-stylus-core/precise-storage -Z stubbing # precise
./verify.sh --precise-storage vault
```

`precise-storage` drops the `to_slot` stubs from every `proof!`, so the SDK
derives each slot itself and hashes it through the keccak oracle — today's
model, on the cheaper store. `precise_proof!` pins a single harness to it.
The whole run uses one model, setup included: Halmos found that mixing layouts
between setup and proof gives wrong results ([Prior art](#prior-art)).

What precise mode checks that structured mode does not is the SDK itself: that
`to_slot` builds each preimage correctly and injectively. That is the storage
nuance a user should not have to re-verify per harness — but it is checkable,
and a release should check it.

### Arbitrary storage (API)

`SymbolicVm::with_arbitrary_storage()`: unwritten slots read as an arbitrary
value, fixed per slot, instead of zero. A harness then starts from every state
of the contract at once — the pre-state of an inductive step — instead of
seeding each field by hand. It is what Certora does at the start of every
rule — "all variables are havoced to model an unknown initial state"
([glossary](https://docs.certora.com/en/latest/docs/user-guide/glossary.html#term-havoc))
— and what hevm's `AbstractStore` and Kontrol's `setArbitraryStorage` provide.

Small slots start as one arbitrary array. A large slot's value is drawn on its
first read and recorded, so later reads agree; each such read therefore uses
one of the `SLOTS`. The states include unreachable ones — `total` below the sum
of balances, say — so a lemma that needs an invariant must assume it.

## Using it

- Wrap mapping harnesses in `proof!`; run with `-Z stubbing`.
- Pre-state: `with_arbitrary_storage()`, then read what the lemma needs before
  taking a `snapshot()`.
- Non-vacuity: a `kani::cover!` on the interesting case, as ever — e.g.
  `vm.slots_changed_since(&before) == 3`. `vm.entries_derived()` and
  `vm.hashes_taken()` show which model a harness actually went through.
- Before trusting a release, run the suite once with `precise-storage` —
  on a machine with room for it: its arbitrary-storage lemmas are the
  expensive ones (see [Measured](#measured)).

## Measured

2026-09-30, one run each, `--no-assertion-reach-checks`, each harness alone in
its own cgroup. Memory caps were sized to what was free — 4.0 to 5.0 GiB —
and each run was limited to 15 minutes. *Old* is `main` at `82da45b`, run
from a worktree in the same session.

**The ladder, on the shipped code.** Time / clauses; every harness's cover
satisfied.

| Rung | Old | Precise | **Structured** |
| --- | --- | --- | --- |
| four `U256` fields | 32.8s / 4.94M | 8.0s / 0.35M | same — no mappings |
| three packed fields | 45.7s / 4.45M | 5.9s / 0.24M | same |
| one symbolic key | 30.2s / 2.19M | 9.7s / 0.47M | **4.9s / 0.20M** |
| two symbolic keys | 66.3s / 4.30M | 36.2s / 1.63M | **10.1s / 0.36M** |
| three symbolic keys | 130.2s / 6.96M | 82.8s / 3.28M | **18.1s / 0.86M** |
| nested map, one entry | 38.5s / 2.95M | 27.1s / 1.03M | **6.2s / 0.24M** |

**`examples/vault`, every harness.** Time · clauses · peak RSS. The first
seven have the same text in all three columns; the conservation lemmas moved
from hand-seeded state to `with_arbitrary_storage()`, and the allowance ones
are new (the old column ran them against `main`'s model, seeded by hand).

| Harness | Old | Precise | **Structured** |
| --- | --- | --- | --- |
| `only_owner_can_transfer_ownership` | 53s · 5.5M · 1.3 GiB | 6s · 0.2M · 0.5 GiB | 7s · 0.2M · 0.5 GiB |
| `owner_can_transfer_ownership` | 52s · 5.5M · 1.2 GiB | 6s · 0.2M · 0.5 GiB | 5s · 0.2M · 0.5 GiB |
| `ownership_cannot_be_claimed_twice` | 36s · 5.4M · 1.2 GiB | 5s · 0.2M · 0.5 GiB | 4s · 0.2M · 0.5 GiB |
| `credit_then_read_roundtrips` | 56s · 6.6M · 1.4 GiB | 24s · 0.7M · 0.5 GiB | 6s · 0.3M · 0.5 GiB |
| `distinct_accounts_do_not_alias` | 226s · 14.7M · 3.5 GiB | 91s · 3.7M · 1.5 GiB | 22s · 0.9M · 0.5 GiB |
| `credit_can_silently_wrap` (finds the bug) | 139s · 12.8M · 2.8 GiB | 53s · 2.9M · 1.1 GiB | 16s · 0.5M · 0.5 GiB |
| `credit_checked_never_wraps` | 290s · 13.3M · 3.0 GiB | 61s · 3.2M · 1.2 GiB | 22s · 0.7M · 0.5 GiB |
| `credit_checked_moves_total_by_the_same_delta` | 197s · 6.0M · 1.9 GiB | 76s · 5.8M · 1.6 GiB | 45s · 2.2M · 0.8 GiB |
| `transfer_conserves_total` | did not finish | did not finish | **150s · 6.7M · 2.3 GiB** |
| `transfer_does_not_move_any_other_balance` | 371s · 8.4M · 2.9 GiB | 119s · 7.3M · 2.4 GiB | 41s · 2.4M · 0.9 GiB |
| `approve_sets_exactly_one_allowance` | — | 42s · 2.2M · 0.9 GiB | 14s · 0.7M · 0.5 GiB |
| `transfer_from_conserves_total` | did not finish | did not finish | **172s · 6.0M · 2.4 GiB** |
| `transfer_from_spends_exactly_the_allowance` | did not finish | did not finish | **132s · 7.3M · 2.4 GiB** |
| `transfer_from_moves_nothing_else` | did not finish | did not finish | **167s · 5.7M · 2.3 GiB** |

*Did not finish* means killed at 15 minutes inside the memory cap. That is
not a time: on this laptop the old model's mapping lemmas needed more memory
than could be spared — `transfer_conserves_total` took 495s inside a 10 GiB
cap on 2026-09-10 (with reachability checks, [50-feasibility.md](50-feasibility.md)),
and 6.4 GiB was seen in `cbmc` on it the day before.

**Seeded or arbitrary pre-state, under precise storage.** The conservation
lemmas changed text, so they were also run as `main` wrote them — seeded by
hand, `SymbolicVm::<4>` — against the new crate, which for that text means
the precise model:

| Harness, `main`'s text | Old crate | New crate |
| --- | --- | --- |
| `credit_checked_moves_total_by_the_same_delta` | 197s · 6.0M · 1.9 GiB | 60s · 2.2M · 1.2 GiB |
| `transfer_does_not_move_any_other_balance` | 371s · 8.4M · 2.9 GiB | 157s · 5.0M · 2.1 GiB |
| `transfer_conserves_total` | did not finish | 193s · 5.6M · 2.5 GiB |

So the encodings alone are worth 2.4–3.3× here too, and bring
`transfer_conserves_total` inside the cap. But note the last row against the
main table: with keccak digests, the **arbitrary-storage** version of that
lemma did not finish where the seeded one takes 193s. Arbitrary storage is
cheap with structured slots and expensive with digests — every first read of
a hashed slot records a fresh value against a fresh 256-bit key. A precise
check of the arbitrary-storage lemmas needs a bigger budget than this laptop,
or seeded twins.

Three things follow:

1. **The encodings alone** — same harness text, old against precise — are
   worth 2.3–9× in time and 4–27× in clauses. That includes proofs with no
   mapping at all: the access-control harnesses, whose cost was almost
   entirely the old host.
2. **The abstraction** — precise against structured — is worth another
   1.7–4× on every harness that touches a mapping, and it is the difference
   between finishing and not on the four heaviest.
3. **The allowance lemmas are new territory.** Three mapping entries, a
   nested map, arbitrary pre-state, all callers: 2–3 minutes and 2.4 GiB each.
   In one harness together, the four identities of lemmas 4 and 5 ran past 15
   minutes even structured, so they are split — the arithmetic, not storage,
   is what is left.

**The other examples, no mappings.** The encodings apply everywhere: the
counter's seven harnesses went from 137s to 31s in total (e.g.
`mul_number_can_wrap` 32.8s · 5.2M clauses → 8.4s · 0.9M), and vesting's
`cannot_reinitialize` from 105s · 8.7M to 20s · 0.7M,
`vested_is_monotone_in_time` from 102s to 71s, and
`vested_unchecked_overflows` from 415s to 171s. All 7 counter, 13 vesting and
14 vault harnesses end as they should, the six `should_panic` ones included.

**With Kani's defaults.** Everything above is without assertion-reachability
checks. With them — `verify.sh`'s flags until 2026-10-01 — `transfer_conserves_total` did not
finish inside a 4.7 GiB cap: the solver was done (UNSAT) but processing the
results stalled at the memory limit, 6 minutes of CPU in 15 of wall time and a
3.7 GiB peak. The heaviest harnesses need either more memory than this laptop
spares or `--no-assertion-reach-checks`; see [Method notes](#method-notes).

**So `verify.sh` now passes `--no-assertion-reach-checks`** (2026-10-01).
Recording the demo, `./verify.sh vault` with the checks on stalled twice:
85 minutes with no progress on `approve_sets_exactly_one_allowance` under a
6 GB `MemoryHigh` (alone, with 7.6 GB, it verified in 137.7s against 14s
without the checks), then `credit_checked_moves_total_by_the_same_delta`
climbing from 2.4 to 7.7 GB in two minutes under a 7.6 GB one. The checks
change no verdict, since Kani counts an unreachable assertion as passing; they
only list which assertions were vacuous. Three vault lemmas guard against
that themselves with `kani::cover!`. The others don't.

With the flag, the same day: `./verify.sh counter` 7 of 7, every harness
under 8s; `./verify.sh vault` 14 of 14 in 645s, peak 2.4 GB, no throttling.
Slowest were the `transfer_from` lemmas, 105–122s.

## Kani limits that shaped this

Each measured on Kani 0.67.0 in a scratch crate.

- **Generic trait methods cannot be stubbed.** The obvious place for a
  value-level model is `GlobalStorage::get_uint::<B, L>` and friends, where
  typed values meet words. Kani refuses: *"Kani does not currently support
  stubs or function contracts on generic functions in traits."*
  ([model-checking/kani#1997](https://github.com/model-checking/kani/issues/1997)).
  Kani 0.68.0 adds trait-impl stubs
  ([#4587](https://github.com/model-checking/kani/pull/4587)) but still not a
  default method its impl doesn't override
  ([#4588](https://github.com/model-checking/kani/issues/4588)), which
  `get_uint` is — so the SDK's byte packing stays in the formula.
- **Methods of generic impls crash the compiler.** Stubbing
  `<ruint::Uint as StorageKey>::to_slot`, with or without explicit
  `<256, 4>`, and `<FixedBytes<32> as StorageKey>::to_slot`, ends in an
  internal compiler error: *"cannot find `BITS/#0` in param-env"*. Hence no
  structured slots for `U256` and `B256` keys. Untried on 0.68.0.
- **What does work:** non-generic trait methods
  (`<StorageCache as GlobalStorage>::get_word`, `<Address as
  StorageKey>::to_slot`) and inherent methods of generic structs
  (`StorageMap::get`, as `ruint::Uint::wrapping_mul` already showed). But the
  SDK's storage types keep `slot` private, so a stub of `StorageUint::get`
  could not find the field it replaces.
- **About fourteen Kani attributes per harness is the ceiling.** Each
  `#[kani::stub]` expands one level deeper than the last; fifteen stubs
  exhaust rustc's default `recursion_limit` of 128 (*"recursion limit reached
  while expanding `#[kanitool::stub]`"*), thirteen fit. `proof!` therefore
  attaches nine, leaving room for `should_panic` and the two `arith_oracle`
  stubs; signed-integer keys, which [`slots`](../crates/kani-stylus-core/src/slots.rs)
  has stubs for, are left out.
- **Stub paths resolve against crates the harness's crate uses.** Kani looks up
  a path's first segment among `tcx.used_crates()`; in a crate that never
  names `stylus_sdk`, `stylus_sdk::…` does not resolve (*"unable to find
  `stylus_sdk` inside module"*). Never an issue in a real contract.

## Assumptions, all together

1. **Distinct mapping entries never share a slot**, and no offset into one
   entry reaches another — in both models; structured mode states it
   directly, the keccak oracle by digest distinctness. As for keccak before,
   the price is that genuine storage-collision attacks are out of scope.
2. **Digests avoid small slots and the structured space** — the oracle's
   `MIN_DIGEST_SLOT` and `TAG` assumptions.
3. **Bounds**: `SLOTS` large slots, `slots::MAX_ENTRIES` (16) structured
   entries and `SMALL_SLOTS` direct cells per proof. Exceeding `SLOTS` or
   `MAX_ENTRIES` fails the proof; beyond `SMALL_SLOTS`, fields go to the list.
   Only the keccak oracle's `MAX_HASHES` still prunes.
4. **With arbitrary storage**, the start state is any state, reachable or not.

## Prior art

Every mature EVM prover does this same split — a flat, precise `slot → word`
map as ground truth, decomposed per logical variable on top — and each keeps a
switch back to the flat model. We found no published measurement of the gain
— hevm calls it "huge", Certora "essential" — which makes the tables above
the only numbers we know of. Researched 2026-09-30; every link returned 200,
and the code links are pinned to a commit.

- **Certora Prover.** Storage analysis plus *storage splitting*: one variable
  per storage path. "For scaling SMT solving to larger programs, these
  simplifications are essential"
  ([techniques](https://docs.certora.com/en/latest/docs/prover/techniques/index.html#analysis-of-evm-storage-and-evm-memory)).
  The precise switch is `--prover_args '-enableStorageSplitting false'`
  ([options](https://docs.certora.com/en/latest/docs/prover/cli/options.html#enablestoragesplitting)).
  When the analysis fails — usually inline assembly — it falls back to the
  flat map *for that contract* and warns "This might have an impact on running
  times"
  ([`StorageSplitter.kt` L83–90, L143–151](https://github.com/Certora/CertoraProver/blob/63fdea80a35b36ebfe0cff5dff9009b90e91966c/src/main/kotlin/analysis/split/StorageSplitter.kt#L143-L151)).
  Keccak is "an arbitrary function that is _injective with large gaps_"
  ([hashing](https://docs.certora.com/en/latest/docs/prover/approx/hashing.html#modeling-the-keccak-function-bounded-case)).
  No paper describes the storage analysis: the OOPSLA 2024 memory-splitting
  paper says "We look forward to presenting and evaluating that in future
  work" ([PDF](https://cnandi.com/docs/oopsla24-cr.pdf), p. 356:27).
- **hevm.** `decomposeStorage` splits storage into one SMT array per keccak
  base when every access matches a known shape
  ([`Expr.hs` L1134–1212](https://github.com/argotorg/hevm/blob/c39757a24425bbc9c56b2c40a5d45faf4bfe70f4/src/EVM/Expr.hs#L1134-L1212)),
  and falls back to the flat array for the whole query otherwise
  ([`SMT.hs` L114–129](https://github.com/argotorg/hevm/blob/c39757a24425bbc9c56b2c40a5d45faf4bfe70f4/src/EVM/SMT.hs#L114-L129));
  `--no-decompose` turns it off. The PR that introduced it claims "a huge
  speedup on all storage related operations"
  ([#436](https://github.com/argotorg/hevm/pull/436)), with no number. Two
  decomposition bugs were fixed this month
  ([#1094](https://github.com/argotorg/hevm/pull/1094)) — the reason our store
  has refinement proofs.
- **Halmos.** `--storage-layout solidity` (the default) decodes
  `keccak(key ‖ slot)` back into per-variable arrays; `generic` replaces
  keccak with a "simple injective function for collision-free (though not
  secure) hash semantics"
  ([`sevm.py` L2026–2029](https://github.com/a16z/halmos/blob/079bb4241d1b460baf986257d56ea86977d73451/src/halmos/sevm.py#L2026-L2029))
  — the closest analogue of our structured slots. No automatic fallback, and
  mixing the two layouts between setup and test gave wrong results
  ([#208](https://github.com/a16z/halmos/issues/208)).
- **Kontrol / KEVM.** Storage stays a flat K map, and its keccak lemmas rest
  on the hypothesis that "the storage slots of a given mapping are presumed to
  be disjoint from slots of other mappings"
  ([`keccak.md`](https://github.com/runtimeverification/kontrol/blob/75bb958ddcebf9f2f0ff5aaa5712831303065612/src/kontrol/kdist/keccak.md)).
  Arbitrary storage is the `setArbitraryStorage` cheatcode.
- **Source-level tools** have no slots at all: the Solidity SMTChecker encodes
  a mapping as an SMT array
  ([docs](https://docs.soliditylang.org/en/latest/smtchecker.html#smt-encoding-and-types)),
  and the Move Prover gives each resource type its own memory
  ([TACAS 2022](https://arxiv.org/abs/2110.08362)). That is the model a
  typed-value stub would have given us, had Kani allowed it.
- **Rust contracts.** OtterSec's stellar-verify, Kani on Soroban, is the
  nearest precedent: a replacement SDK with storage in a fixed 10-entry
  association list
  ([`storage.rs`](https://github.com/otter-sec/stellar-verify/blob/f4e7a2a2563ab46d70b9185e51d584022ec8beb3/stellar/soroban-env-common/src/storage.rs)).
- **No formal verifier for Stylus was found.** Certora's WebAssembly front end
  supports Soroban and NEAR hosts only
  ([`Converter.kt` L370–375](https://github.com/Certora/CertoraProver/blob/63fdea80a35b36ebfe0cff5dff9009b90e91966c/lib/GeneralUtils/src/main/kotlin/cli/Converter.kt#L370-L375));
  Runtime Verification's Skribe has Stylus semantics but is a fuzzer
  ([final report](https://forum.arbitrum.foundation/t/skribe-advanced-fuzzing-for-stylus-final-milestone-report/30984)).

## Method notes

- **Most of Kani's default "verification time" is not the model.** With
  assertion-reachability checks on, Kani's reported time on a 4-field harness
  was 93s; CBMC's own phases added up to about 33s, and the rest went on
  turning the reachable-check traces into results. `--no-assertion-reach-checks`
  gave 10.5s against 49s on a single field, and halved peak memory. So the
  ladder compares models with the checks off, and `kani::cover!` does the
  non-vacuity job instead. `verify.sh` still keeps them.
- One sample per point. Earlier repeats on this machine agreed to a few
  percent when idle; see [50-feasibility.md](50-feasibility.md).

## Not done yet

In rough order of value:

1. **`U256` and `B256` keys** — ERC-721 token ids, role ids. Blocked by the
   compiler crash above; worth retrying on Kani 0.68.
2. **Sequences of calls from arbitrary storage** — the inductive step of
   roadmap item 2 now has its pre-state; the invariant, and machine-checking
   the induction, remain.
3. **A per-contract fallback, as Certora has.** Today the whole run is one
   model or the other, by the user's choice.
4. **An attribute macro** (`#[kani_stylus::proof]`) instead of `proof! { }`,
   which rustfmt and rust-analyzer handle less well inside the braces.
5. **Structured slots for `StorageVec` and `StorageBytes` elements**, which
   hash through `crypto::keccak` directly.
6. **Typed values**, skipping the SDK's byte packing — blocked by Kani's
   generic-trait-method limit, and worth less than it looked (about 20k
   variables per access).
