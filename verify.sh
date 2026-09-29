#!/usr/bin/env bash
# Run the kani-stylus proof suites.
#
#   ./verify.sh                      every example
#   ./verify.sh counter              just examples/counter
#   ./verify.sh vault                just examples/vault
#   ./verify.sh vesting              just examples/vesting
#   ./verify.sh -h <harness>         one harness (searches all)
#   ./verify.sh --playback <harness> print the counterexample as a runnable test
#
# Each example is a standalone, deployable Stylus project, so proofs are behind
# an opt-in `proofs` feature and ordinary builds are untouched.
#
# The vault and vesting both need `-Z stubbing`, for different reasons: the
# vault because Stylus mappings hash through `stylus_sdk::crypto::keccak`
# rather than through the `Host` trait, vesting because a symbolic `U256`
# division has to be replaced by its specification or the proof diverges.
#
# vesting also gates its slowest and non-converging harnesses behind a
# `slow-proofs` feature, which this script deliberately does not enable.
set -euo pipefail
cd "$(dirname "$0")"

COMMON=(--features proofs --output-format terse)
COUNTER=(--manifest-path examples/counter/Cargo.toml "${COMMON[@]}")
VAULT=(--manifest-path examples/vault/Cargo.toml "${COMMON[@]}" -Z stubbing)
VESTING=(--manifest-path examples/vesting/Cargo.toml "${COMMON[@]}" -Z stubbing)

case "${1:-all}" in
  --playback)
      [[ -n "${2:-}" ]] || { echo "usage: $0 --playback <harness>" >&2; exit 2; }
      cargo kani "${COUNTER[@]}" -Z concrete-playback --concrete-playback=print \
          --harness "$2" && exit 0
      cargo kani "${VAULT[@]}" -Z concrete-playback --concrete-playback=print \
          --harness "$2" && exit 0
      exec cargo kani "${VESTING[@]}" -Z concrete-playback --concrete-playback=print \
          --harness "$2" && exit 0 ;;
  -h) [[ -n "${2:-}" ]] || { echo "usage: $0 -h <harness>" >&2; exit 2; }
      cargo kani "${COUNTER[@]}" --harness "$2" && exit 0
      cargo kani "${VAULT[@]}" --harness "$2" && exit 0
      exec cargo kani "${VESTING[@]}" --harness "$2" && exit 0 ;;
  counter) exec cargo kani "${COUNTER[@]}" ;;
  vault)   exec cargo kani "${VAULT[@]}" ;;
  vesting) exec cargo kani "${VESTING[@]}" ;;
  all)     ;;
  *)       echo "unknown target: $1" >&2; exit 2 ;;
esac

echo "### examples/counter  — storage, arithmetic, payable"
cargo kani "${COUNTER[@]}"
echo
echo "### examples/vault  — access control and mappings"
cargo kani "${VAULT[@]}"
echo
echo "### examples/vesting"
cargo kani "${VESTING[@]}"
