# kani-smoke

The feasibility spike behind [`../../kb/50-feasibility.md`](../../kb/50-feasibility.md).
It answers three questions, in order:

1. Can Kani verify a Stylus contract at all?
2. What does it cost?
3. Will it actually catch a bug?

## Harness ladder

Three groups, each adding one layer so cost cliffs can be located rather than
guessed at.

| Prefix | Host | Purpose |
| --- | --- | --- |
| `h*` | `TestVM` (the SDK's own mock) | baseline — **fails**, see below |
| `s*` | `SymbolicVM` (ours) | the working path |
| `d*` | `SymbolicVM` | defect detection and counterexamples |

```bash
cargo kani --harness s4_set_then_get --output-format terse   # one harness
cargo kani --output-format terse                             # all of them

# turn a counterexample into a runnable #[test] (needs the unstable flag)
cargo kani -Z concrete-playback --concrete-playback=print \
    --harness d1_add_number_can_decrease_the_counter
```

## Results (2026-09-08, Kani 0.67.0, stylus-sdk 0.10.9)

**`TestVM` cannot be verified.** `h1_vm_only` — merely `TestVM::default()` —
fails in 2.4s of solver time:

```
call to foreign "C" function `syscall` is not currently supported by Kani
  in std::sys::random::linux::getrandom::getrandom
```

Its `VMState` holds nine `std::HashMap`s, and `HashMap::new()` seeds SipHash
from OS randomness. `h2_contract_only` then times out at 420s.

**`SymbolicVM` works and is fast.** Same properties, array-backed storage:

| Harness | Checks | Time |
| --- | --- | --- |
| `s1_contract_only` | 348 | 6s |
| `s2_read_only` | 538 | 8s |
| `s3_starts_at_zero` | 539 | 8s |
| `s4_set_then_get` | 1044 | 14s |
| `s5_add_no_overflow` | 1044 | 20s |
| `s6_symbolic_ctx` | 1047 | 26s |

`s4` is the harness that timed out at 25 minutes against `TestVM`.

**And it catches real bugs.** `d1` shows `add_number` in the stock Stylus
counter template can *decrease* the counter, because `alloy`'s `U256 + U256` is
`wrapping_add` and never panics. Kani's automatic overflow checks do not cover
it — `U256` is a library type over explicitly-wrapping `u64` limbs. Overflow has
to be asserted explicitly; see `d3` for the shape a real proof obligation takes.
