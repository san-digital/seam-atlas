#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LEAN_SRC="$REPO_ROOT/lean"

if [[ -n "${LEAN_BIN_DIR:-}" ]]; then
  export PATH="$LEAN_BIN_DIR:$PATH"
fi

if [[ -z "${MATHLIB_DIR:-}" ]]; then
  echo "error: set MATHLIB_DIR to a compiled mathlib4 directory" >&2
  exit 1
fi

BUILD_DIR="$(mktemp -d "${TMPDIR:-/tmp}/seam-atlas-lean.XXXXXX")"
trap 'rm -rf "$BUILD_DIR"' EXIT
mkdir -p "$BUILD_DIR/SeamAtlas"

cd "$MATHLIB_DIR"

lake env bash -c '
  set -euo pipefail
  export LEAN_PATH="'"$BUILD_DIR"':${LEAN_PATH:-}"
  for mod in PartitionSoundness CliffEdge; do
    echo "[lean] compiling SeamAtlas.${mod}"
    lean -R "'"$LEAN_SRC"'" -o "'"$BUILD_DIR"'/SeamAtlas/${mod}.olean" "'"$LEAN_SRC"'/SeamAtlas/${mod}.lean"
  done

  echo "[leanchecker] replaying SeamAtlas modules"
  leanchecker -v SeamAtlas.PartitionSoundness SeamAtlas.CliffEdge

  echo "[axioms] checking foundational axiom footprint"
  AXIOM_OUT="$(lean --stdin <<'"'"'EOS'"'"'
import SeamAtlas.CliffEdge
#print axioms SeamAtlas.pipeline_eval_eq_on_cell
#print axioms SeamAtlas.diff_sound_and_complete
#print axioms SeamAtlas.diff_witness_complete
#print axioms SeamAtlas.exists_outcome_flip_on_walk
#print axioms SeamAtlas.exists_large_severity_step
#print axioms SeamAtlas.unbuffered_cliff_of_missing_intermediate
EOS
)"
  echo "$AXIOM_OUT"
  # Long axiom lists wrap across lines, so join the output before extracting each list.
  # Anything outside the three standard axioms (sorryAx, Lean.ofReduceBool from
  # native_decide, or a custom axiom) fails the audit.
  NONSTANDARD="$(printf "%s\n" "$AXIOM_OUT" | tr "\n" " " | grep -o "\[[^]]*\]" | tr -d "[]" \
    | tr "," "\n" | sed "s/^ *//; s/ *$//" \
    | grep -v -x -e "" -e "propext" -e "Classical.choice" -e "Quot.sound" || true)"
  if [[ -n "$NONSTANDARD" ]]; then
    echo "error: non-standard axioms in footprint: $(echo $NONSTANDARD)" >&2
    exit 1
  fi
'

echo "[ok] all SeamAtlas Lean 4 modules compiled, kernel-replayed, and axiom-audited"
