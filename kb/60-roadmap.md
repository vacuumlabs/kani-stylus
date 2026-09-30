# Roadmap (tentative)

Written 2026-09-09, after the example suites landed. **This is a judgement
call, not a commitment** — the sequencing follows the measured cost curve in
[50-feasibility.md](50-feasibility.md), and it should be re-cut whenever a
measurement moves. Numbered open questions referenced below are that file's.

- [00-project.md](00-project.md) — what's **done** (status checklist)
- [50-feasibility.md](50-feasibility.md) — what's **unknown** (open questions)
- this file — what's **next**, and in what order

## The one-line summary

Today kani-stylus is usable by its authors on scalar-storage properties and on
conservation properties over mappings, at **under ten minutes per harness** —
the full 17-harness suite runs in 46 minutes. It is not yet usable by anyone
else on anything: there is no walkthrough, no reusable property library, and
nothing has been verified that we did not write ourselves. Everything below is
ordered by what it takes to change that second sentence.

## Now (days) — close out the MVP

- [x] ~~A single command that runs the whole suite~~ — [`verify.sh`](../verify.sh)
      does this, including `--playback`. The 00-project status item is stale on
      this half.
- [ ] **"Write your first proof" walkthrough.** README §4 links out to upstream
      Stylus and Kani docs; there is still nothing that walks a developer from
      their own contract to a first passing harness. This is the cheapest
      adoption win available and needs no new code.
      *Done when:* someone who has never used Kani can add a proof to a contract
      we didn't write, following only the walkthrough.

## Weeks — make it usable by someone else

Ordered by dependency. **Re-cut 2026-09-09** after the conservation work below:
the mapping optimisation turned out *not* to gate conservation, only its pace
and its reach.

0. [x] ~~**Conservation on our own ERC-20-shaped contract.**~~ — **Done
       2026-09-09.** `examples/vault` gained `transfer`, and three local-delta
       lemmas verify. The literal summed property (`total == Σ balance(aᵢ)`) was
       abandoned as unstatable in a BMC; conservation is decomposed into
       per-method delta + frame lemmas instead, with the induction over call
       sequences left as a disclosed hand argument. New API:
       `vm.snapshot()` / `vm.slots_changed_since()`. See
       [50-feasibility.md](50-feasibility.md).
1. [x] ~~**Make mapping proofs cheap.**~~ — **Largely done 2026-09-10.** The
       17-harness suite went from ~125 min to **46 min**, and the worst harness
       from 1904s to 359s, by storing the keccak oracle's memo table as 256-bit
       words instead of byte arrays. Model-neutral. Four other candidates were
       each worth <=10%. Remaining if more is ever needed: the two-tier slot
       store, and narrowing balances to `u64` shapes (untested). Memory, not
       time, is now clearly the binding constraint.
       Two lessons worth keeping from getting there: `SLOTS`, `MAX_HASHES` and
       digest width are *size* levers, and size governs memory and encodability
       rather than speed — so rank time work by **real solves only**, never by
       formula size, which was tried as a fast proxy and is invalid here.
       **Re-cut 2026-09-30:** the storage model itself was the rest. A
       zero-sized host, a two-tier store and structured mapping slots — the
       last behind a `precise-storage` flag — cut three symbolic keys from
       130s to 21s and made ERC-20 `transfer_from` provable from an arbitrary
       state. See [36-storage-model.md](36-storage-model.md).
2. [ ] **Properties over sequences of calls.** *Closes Q9.*
       Every harness today proves one method call from a hand-havoc'd state, but
       "no sequence of calls breaks this" is the property contract authors
       actually want. Two routes, and we probably want both:
       a bounded symbolic-action dispatcher (no invariant needed, depth-limited),
       and the inductive form (base case + step case from arbitrary state
       satisfying the invariant — unbounded, but needs the invariant found and
       usually strengthened).
       The arbitrary pre-state now exists —
       `SymbolicVM::with_arbitrary_storage()`, used by the vault's conservation
       lemmas since 2026-09-30.
       *Done when:* one vault property holds over all interleavings of ≥3 calls.
3. [ ] **A property library.** The crate README pitches one; the reality is
       hand-written harnesses per contract. Reusable templates — conservation,
       access control, monotonicity, no-aliasing, panic freedom — instantiable
       against a contract's methods.
       *Done when:* a new contract gets a meaningful proof suite by naming its
       methods, not by writing SMT-shaped Rust.
4. [ ] **Verify a contract we didn't write.** *Closes Q5.*
       OZ's ERC-20 keys its maps by `Address`, which structured slots cover;
       an ERC-721's `U256` token ids are not covered yet (item 9).
       OpenZeppelin [`rust-contracts-stylus`](https://github.com/OpenZeppelin/rust-contracts-stylus)
       ERC-20. **No longer blocked on item 1** — item 0 shows the local-delta
       formulation works at current speed, so the question is now whether OZ's
       abstraction depth (not the mapping cost) defeats the solver. The
       hand-rolled fallback is effectively already done as `examples/vault`.
       *Done when:* an audited third-party contract verifies unmodified, or we
       can say precisely why it doesn't.

## Months — make it a product

5. [ ] **Modular verification via Kani function contracts.** The principled way
       past the scale wall: prove a method against a `requires`/`ensures` spec
       once, then *reuse the spec* at call sites instead of re-verifying the
       body. Kani ships this as experimental (`-Z function-contracts`), so this
       is partly a bet on upstream. Likely the highest-ceiling item here, and
       the one most likely to turn "weeks of tuning" into "it composes".
6. [ ] **Calldata-level proofs.** *Closes the open half of Q7.*
       Proofs today call methods directly, sidestepping the ABI router. "Panic
       freedom over arbitrary calldata" — a proposal claim — requires proving
       *through* `#[public]`'s generated router from a symbolic byte string.
       Strictly harder, and the honest writeup must keep saying "method-level"
       until this exists.
7. [ ] **Cross-contract calls and reentrancy.** Proposal Phase 1. `call_contract`
       and friends are `unimplemented!()` today, so a proof touching them fails
       loudly. Reentrancy is the property that would make this land with
       auditors — and it is inherently a multi-call property, so it depends on
       item 2, not just on modelling the call.
9. [ ] **Structured slots for `U256` and `B256` keys.** Kani 0.67 crashes when
       asked to stub a method of a generic impl, which is what their
       `StorageKey::to_slot` is; until then they hash through the keccak oracle
       at the old per-key price. Retry on Kani 0.68. See
       [36-storage-model.md](36-storage-model.md#kani-limits-that-shaped-this).
8. [ ] **CI integration.** Proposal Phase 3. A GitHub Action running the suite
       per pull request. **Deliberately last:** the vault suite is 3155s today,
       so this is gated on item 1 rather than on any CI work. Shipping it before
       proofs are fast produces a red, ignored badge.

## Why not the proposal's phases

[proposal.md](proposal.md) §6 sequences the post-hackathon work as
cross-contract calls and reentrancy (Months 1–2), Solidity-proxy storage layout
compatibility (Months 3–4), then CI (Months 5–6). Three problems, all of which
only became visible after measuring:

- **It front-loads cross-contract calls** while single-contract mapping
  properties still don't converge. Reentrancy also presupposes multi-call
  reasoning, which does not exist yet in any form.
- **It puts CI at the end but treats it as independent.** CI is not
  months-of-work; it is days of work gated on proof speed.
- **Proxy storage-layout compatibility is absent from our list entirely.** It's
  a coherent idea and genuinely differentiating, but it's a niche next to
  "prove an ERC-20", and nothing measured so far argues for it. Revisit if a
  user asks.

Keep the proposal's out-of-scope list (full dynamic ABI decoding, AST linting) —
still sensible.

## Deliberately not on the roadmap

- **Upstream SDK contributions.** A `mock-host` feature split (the
  `Box<dyn Host>` VM without `dep:stylus-test`) would cut build times and make
  intent explicit. It stays a documented option — see
  [50-feasibility.md](50-feasibility.md). Everything works without it and the
  cost is compile time only. Consistent with
  [00-project.md](00-project.md#deliberately-not-doing).
- **Verifying the compiled WASM.** Kani runs natively against Rust source. It
  says nothing about miscompilation or ArbOS semantics. That is a permanent,
  stateable limitation, not a gap to close.
