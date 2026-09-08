#!/usr/bin/env bash
# Run the kani-stylus proof suites.
#
#   ./verify.sh                      every harness in every project
#   ./verify.sh counter              just stylus-samples/counter
#   ./verify.sh examples             just examples/proofs
#   ./verify.sh -h <harness>         one harness (searches both projects)
#   ./verify.sh --playback <harness> print the counterexample as a runnable test
#
# Note `--features proofs` on the counter: that project is a real, deployable
# Stylus contract, so the verification dependencies are opt-in and its ordinary
# build is untouched. See stylus-samples/counter/Cargo.toml.
set -euo pipefail
cd "$(dirname "$0")"

COUNTER=(--manifest-path stylus-samples/counter/Cargo.toml --features proofs)
EXAMPLES=(--manifest-path examples/proofs/Cargo.toml)
COMMON=(-Z stubbing --output-format terse)

case "${1:-all}" in
  --playback)
      [[ -n "${2:-}" ]] || { echo "usage: $0 --playback <harness>" >&2; exit 2; }
      exec cargo kani "${EXAMPLES[@]}" -Z stubbing -Z concrete-playback \
          --concrete-playback=print --harness "$2" ;;
  -h) [[ -n "${2:-}" ]] || { echo "usage: $0 -h <harness>" >&2; exit 2; }
      cargo kani "${COUNTER[@]}"  "${COMMON[@]}" --harness "$2" && exit 0
      exec cargo kani "${EXAMPLES[@]}" "${COMMON[@]}" --harness "$2" ;;
  counter)  exec cargo kani "${COUNTER[@]}"  "${COMMON[@]}" ;;
  examples) exec cargo kani "${EXAMPLES[@]}" "${COMMON[@]}" ;;
esac

echo "### stylus-samples/counter  (a real cargo-stylus project)"
cargo kani "${COUNTER[@]}" "${COMMON[@]}"
echo
echo "### examples/proofs  (counter + vault, mappings and access control)"
cargo kani "${EXAMPLES[@]}" "${COMMON[@]}"
