# How seam-atlas is checked

This page explains why checking one input per region is enough, what the Lean 4 proofs
in [`lean/SeamAtlas/`](lean/SeamAtlas) establish, and what is only covered by tests.

Short version: the Lean proofs cover the method on an abstract model. The Rust code is
not formally verified; it is tested against brute-force enumeration.

## The method

`compile_partition` splits the input space into boxes (one interval, bool subset or enum
subset per field). It keeps splitting until, inside each box, every guard of the policy
is either true for all inputs or false for all inputs. A first-match pipeline then gives
the same outcome to every input in the box, so evaluating one example input per box
decides the outcome for all of them.

`diff_pipelines` does the same with the guards of both policies at once, so each box has
one old outcome and one new outcome. `find_cliffs` looks for pairs of boxes that touch
along one field and have different outcomes.

## What the Lean proofs establish

The model in `PartitionSoundness.lean` is abstract: a rule is a list of predicate ids, a
function `sem` says whether a predicate holds at an input, and a pipeline returns the
outcome of the first rule whose predicates all hold, or a default.

| Theorem | Statement in plain terms |
| :--- | :--- |
| `pipeline_eval_eq_on_cell` | If every predicate a pipeline uses has the same truth value at all points of a set `C`, the pipeline gives every point of `C` the same outcome. |
| `diff_sound_and_complete` | Take a cover of a domain `D` by cells, each with a chosen point inside it, where every cell is predicate-constant for both pipelines. Then the two pipelines agree on all of `D` if and only if they agree on every chosen point. |
| `diff_witness_complete` | Under the same assumptions, if the pipelines disagree anywhere in `D`, they disagree on some chosen point. |
| `exists_outcome_flip_on_walk` | Any chain of steps from one input to an input with a different outcome contains a single step where the outcome changes. |
| `exists_large_severity_step` | If severity rises by at least `k * delta` over a chain of `k` steps, some single step rises by at least `delta`. |
| `unbuffered_cliff_of_missing_intermediate` | If no input has a severity strictly between `low` and `high` (with `high >= low + 2`), any chain from severity at most `low` to severity at least `high` contains a step that rises by 2 or more. |

The last three are general facts about chains of steps. They explain why cliff edges
must exist when outcomes differ, but they say nothing about whether `find_cliffs` reports
all of them.

## What they do not cover

- The Lean model does not include integer intervals, `scaled(...)`, the parser, or any
  of the Rust functions. Nothing links the Lean definitions to the Rust code (no
  extraction and no Rust verification tool).
- In particular, the proofs assume the cells are predicate-constant and cover the
  domain. That the Rust boxes really are disjoint, cover every input, and are
  predicate-constant is checked by tests only.
- The Lean pipeline returns an outcome, not the matching rule, so grouping by rule in
  `diff` is not covered.
- Whether `find_cliffs` finds every one-step boundary is checked by tests only.

## The tests that cover the Rust code

- [`tests/randomized_brute_force.rs`](tests/randomized_brute_force.rs) generates 10,000
  random policy pairs with small domains and compares `compile_partition`,
  `diff_pipelines` and `find_cliffs` with a point-by-point enumeration of every input.
- [`tests/exhaustive_equiv.rs`](tests/exhaustive_equiv.rs) does the same by hand for a
  fixed grid, and checks a field spanning the whole `i64` range for overflow.
- [`scripts/crosscheck.py`](scripts/crosscheck.py) runs the CLI on the example policies
  and re-evaluates every reported input with a separate Python implementation.
- In debug builds, `diff_pipelines` and `find_cliffs` re-evaluate every reported example
  with `Pipeline::eval` (`debug_assert`). CI runs the test suite in debug and release.

## Axioms and replay

The modules compile against Mathlib `v4.33.1`. `lean/replay.sh` compiles them, replays
them in the kernel checker (`leanchecker`), and fails if any theorem depends on an axiom
other than the standard `propext`, `Classical.choice` and `Quot.sound`. That rules out
`sorry`, `native_decide` and custom axioms. The `lean` CI job runs the script on every
push and pull request.

To replay locally you need [elan](https://github.com/leanprover/elan) and a Mathlib
checkout with its build cache:

```sh
git clone --depth 1 --branch v4.33.1 https://github.com/leanprover-community/mathlib4
(cd mathlib4 && lake exe cache get)
MATHLIB_DIR=$PWD/mathlib4 ./lean/replay.sh
```
