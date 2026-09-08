#!/usr/bin/env bash
# Run the kani-stylus proof suites.
#
#   ./verify.sh                      every example
#   ./verify.sh counter              just examples/counter
#   ./verify.sh vault                just examples/vault
#   ./verify.sh -h <harness>         one harness (searches both)
#   ./verify.sh --playback <harness> print the counterexample as a runnable test
#
# Each example is a standalone, deployable Stylus project, so proofs are behind
# an opt-in `proofs` feature and ordinary builds are untouched. The vault also
# needs `-Z stubbing`, because Stylus mappings hash through
# `stylus_sdk::crypto::keccak` rather than through the `Host` trait.
set -euo pipefail
cd "$(dirname "$0")"

COMMON=(--features proofs --output-format terse)
COUNTER=(--manifest-path examples/counter/Cargo.toml "${COMMON[@]}")
VAULT=(--manifest-path examples/vault/Cargo.toml "${COMMON[@]}" -Z stubbing)

case "${1:-all}" in
  --playback)
      [[ -n "${2:-}" ]] || { echo "usage: $0 --playback <harness>" >&2; exit 2; }
      cargo kani "${COUNTER[@]}" -Z concrete-playback --concrete-playback=print \
          --harness "$2" && exit 0
      exec cargo kani "${VAULT[@]}" -Z concrete-playback --concrete-playback=print \
          --harness "$2" ;;
  -h) [[ -n "${2:-}" ]] || { echo "usage: $0 -h <harness>" >&2; exit 2; }
      cargo kani "${COUNTER[@]}" --harness "$2" && exit 0
      exec cargo kani "${VAULT[@]}" --harness "$2" ;;
  counter) exec cargo kani "${COUNTER[@]}" ;;
  vault)   exec cargo kani "${VAULT[@]}" ;;
esac

echo "### examples/counter  — storage, arithmetic, payable"
cargo kani "${COUNTER[@]}"
echo
echo "### examples/vault  — access control and mappings"
cargo kani "${VAULT[@]}"
