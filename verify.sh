#!/usr/bin/env bash
# Run the kani-stylus proof suite.
#
#   ./verify.sh                     every harness in examples/proofs
#   ./verify.sh <harness-substring>  just the matching ones
#   ./verify.sh --playback <harness> re-run one harness and print the
#                                    counterexample as a runnable #[test]
#
# -Z stubbing is always on: the mapping proofs need the keccak stub, and it is
# inert for harnesses that don't use it.
set -euo pipefail

cd "$(dirname "$0")/examples/proofs"

FLAGS=(-Z stubbing --output-format terse)

if [[ "${1:-}" == "--playback" ]]; then
    [[ -n "${2:-}" ]] || { echo "usage: $0 --playback <harness>" >&2; exit 2; }
    exec cargo kani -Z stubbing -Z concrete-playback \
        --concrete-playback=print --harness "$2"
fi

if [[ -n "${1:-}" ]]; then
    exec cargo kani "${FLAGS[@]}" --harness "$1"
fi

echo "Verifying all harnesses. First run compiles the dependency tree and"
echo "takes a few minutes; later runs are much faster."
exec cargo kani "${FLAGS[@]}"
