# Soundness Proof Overview

`seam-atlas` compiles ordered rule pipelines into exact disjoint coordinate partitions without external SMT solver binaries. The Lean 4 modules in [`lean/SeamAtlas/`](lean/SeamAtlas) machine-check the soundness and completeness of both core passes.

## Verified Theorems (`lean/SeamAtlas/`)

All theorems compile against Mathlib4 (`v4.33.1`) and pass standalone kernel replay (`leanchecker -v`) with zero `sorryAx`:

| Module | Theorem | Axiom Footprint | Role in Rust Crate |
| :--- | :--- | :--- | :--- |
| `PartitionSoundness.lean` | `pipeline_eval_eq_on_cell` | `[propext]` | Proves `Pipeline::eval` is invariant across every point of an atomic-predicate-constant box (`LeafRegion`) |
| `PartitionSoundness.lean` | `diff_sound_and_complete` | `[propext]` | Proves `old` and `new` agree on 100% of an input domain `D` if and only if they agree on the single canonical witness of each cell in a pointed cover (`diff_pipelines`) |
| `PartitionSoundness.lean` | `diff_witness_complete` | `[propext]` | Proves that if any input `x in D` changes outcome between `old` and `new`, some cell witness `cr.2 in D` exhibits `old(cr.2) != new(cr.2)` |
| `CliffEdge.lean` | `exists_outcome_flip_on_walk` | `[propext, Classical.choice, Quot.sound]` | Proves every discrete perturbation path between distinct outcomes crosses an adjacent 1-step boundary flip |
| `CliffEdge.lean` | `exists_large_severity_step` | `[propext, Classical.choice, Quot.sound]` | Discrete pigeonhole bound across unit-perturbation chains |
| `CliffEdge.lean` | `unbuffered_cliff_of_missing_intermediate` | `[propext, Classical.choice, Quot.sound]` | Proves that omitting intermediate severity bands between `low` and `high` (`low + 2 <= high`) forces an unbuffered single-step severity jump `>= 2` (`unbuffered_seam = true`) |

## Scope Boundary

- `seam-atlas` is exact for deterministic first-match conjunctive rule pipelines over bounded 64-bit integer intervals (`IntRange`), unit-normalized products (`ScaledIntCmp`), booleans (`Bool`), and finite enumerations (`Enum`).
- Non-linear cross-variable arithmetic (`x * y >= z` where both `x` and `y` are wide continuous integer intervals) or unbounded string regexes lie outside exact orthogonal cell refinement.
- The Lean theorems are stated over an abstract pipeline and an arbitrary predicate-constant pointed cover. They do not model the Rust refinement itself: that `eval_pred_on_box` and `split_box_for_pred` produce a disjoint, predicate-constant cover of the whole schema is checked by [`tests/randomized_brute_force.rs`](tests/randomized_brute_force.rs), which compares `compile_partition`, `diff_pipelines`, and `find_cliffs` against exhaustive enumeration on 10,000 generated policies.

## Replaying Locally

```sh
MATHLIB_DIR=/path/to/mathlib4 ./lean/replay.sh
```

The `lean` CI job runs the same script against Mathlib `v4.33.1` on every push and pull request. The axiom audit accepts only `propext`, `Classical.choice`, and `Quot.sound`, so `sorryAx`, `Lean.ofReduceBool` (`native_decide`), and custom axioms all fail it.
