#!/usr/bin/env bash
# Dump one harness's SMT-LIB 2 formula. Usage: ./dump.sh <harness> [outfile]
#
# --cbmc-args needs -Z unstable-options; --z3 picks the SMT2 backend and
# --outfile makes CBMC write the formula instead of solving it. Kani then
# reports "VERIFICATION:- FAILED" because nothing was solved — ignore it.
set -uo pipefail
H="${1:?usage: ./dump.sh <harness> [outfile]}"
OUT="${2:-./$H.smt2}"
cargo kani -Z unstable-options -Z stubbing --harness "$H" \
  --cbmc-args --z3 --outfile "$OUT" >/dev/null 2>&1
echo "$OUT  ($(wc -l < "$OUT") lines)"
