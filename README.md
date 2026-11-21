# seam-atlas

Spec-free exhaustive behavioral diff and cliff-edge boundary compiler for deterministic rule engines, clinical/safety guardrails, and IAM policies.

Zero runtime dependencies. `#![forbid(unsafe_code)]`. Backed by kernel-replayed Lean 4 soundness and completeness proofs in [`lean/SeamAtlas/`](https://github.com/san-digital/seam-atlas/tree/main/lean/SeamAtlas) (documented in [`PROOF.md`](https://github.com/san-digital/seam-atlas/blob/main/PROOF.md)).

## Installation

```sh
cargo install seam-atlas
```

Or as a library dependency:

```toml
[dependencies]
seam-atlas = "0.1"
```

## Why This Exists

Formal verification tools usually stall in software teams because engineers do not write separate formal specifications, while unit tests and fuzzers sample isolated points and miss narrow multi-condition cracks.

`seam-atlas` requires **zero formal specifications**. It treats a deterministic priority-ordered rule pipeline itself as the mathematical object, refines the input schema into an exact cover of predicate-constant hyper-rectangles (`LeafRegion`), and computes two artifacts automatically:

1. **Minimal-Perturbation Cliff Edges (`seam-atlas cliffs`):** Solves for input pairs `(x, x')` identical on `n - 1` fields where changing one integer by `1` (`3999 mg` -> `4000 mg`), flipping one unit (`4 mg` -> `4 g`), or toggling one state flag flips the pipeline outcome. Flags **unbuffered seams** where severity jumps by `>= 2` levels across a single atomic step without an intermediate warning/abstention band.
2. **Exhaustive Behavioral PR Diffs (`seam-atlas diff`):** Co-refines `old` and `new` rule versions across 100% of their shared domain (billions of states in sub-millisecond time), proving `old(x) == new(x)` everywhere unchanged and emitting one concrete witness per `(old_outcome, new_outcome, old_rule, new_rule)` equivalence class where behavior changed.

## CLI Usage

### 1. Discover Cliff Edges and Unbuffered Seams

```sh
cargo run --release -- cliffs policies/clinical_v1.policy
```

Output on [`policies/clinical_v1.policy`](https://github.com/san-digital/seam-atlas/blob/main/policies/clinical_v1.policy) (`7,260,726` total states):

```text
pipeline 'paracetamol_triage' compiled into 8 disjoint regions (volume = 7260726)
discovered 5 minimal-perturbation cliff edges (5 unbuffered severity jumps >= 2):
  [UNBUFFERED] field=dose delta_sev=3: Routine ({age_years=0, dose=3999, unit=mg, hepatic_risk=false}) -> Emergency ({age_years=0, dose=4000, unit=mg, hepatic_risk=false})
  [UNBUFFERED] field=unit delta_sev=3: Routine ({age_years=1, dose=4, unit=mg, hepatic_risk=false}) -> Emergency ({age_years=1, dose=4, unit=g, hepatic_risk=false})
  ...
```

Add `--json` to emit deterministic CI regression test vectors.

### 2. Exhaustive Behavioral Diff Across Policy Versions

```sh
cargo run --release -- diff policies/iam_v1.policy policies/iam_v2.policy
```

Output on [`policies/iam_v1.policy`](https://github.com/san-digital/seam-atlas/blob/main/policies/iam_v1.policy) vs [`policies/iam_v2.policy`](https://github.com/san-digital/seam-atlas/blob/main/policies/iam_v2.policy) (`22,649,241,816` states, catching a rule-reordering refactor that placed `allow_auditor_read` above `require_prod_mfa`):

```text
CHANGED: 314572803 of 22649241816 states altered across 1 equivalence classes:
  Deny (sev=3) -> Allow (sev=0) [volume=314572803, regions=1] witness={role=auditor, env=prod, action=read, mfa=false, ip_zone=corp, payload_bytes=0}
```

Pass `--fail-on-diff` in CI to gate refactors that claim to be behavior-preserving.

## Library Usage

```rust
use seam_atlas::{diff_pipelines, find_cliffs, parse_policy};

let v1 = parse_policy("
pipeline rate_gate
default Allow severity=0

field req_per_min int 0..10000
field tier enum free,pro
field verified bool

rule hard_block -> Block severity=3:
  tier == free
  req_per_min >= 600
").unwrap();

let cliffs = find_cliffs(&v1);
assert!(cliffs.iter().any(|c| c.field_name == "req_per_min" && c.unbuffered_seam));

// Equivalent rewrite (`>= 600` rewritten as `> 599`) is proved identical across all 40,004 states:
let v2 = parse_policy("
pipeline rate_gate
default Allow severity=0

field req_per_min int 0..10000
field tier enum free,pro
field verified bool

rule hard_block -> Block severity=3:
  req_per_min > 599
  tier == free
").unwrap();

let diff = diff_pipelines(&v1, &v2).unwrap();
assert!(diff.is_equivalent);
assert_eq!(diff.total_volume, 40_004);
```

## Policy Syntax

- `pipeline <name>`
- `default <Label> severity=<0..255>`
- `field <name> int <min>..<max>` | `field <name> bool` | `field <name> enum <v1>,<v2>,...`
- `rule <id> -> <Label> severity=<0..255>:` followed by conjunctive guards:
  - Integer: `<field> == <n>`, `!=`, `<`, `<=`, `>`, `>=`, `<field> in <lo>..<hi>`
  - Unit-scaled integer: `scaled(<val_field>, <unit_field>, <u1>=<s1>, ...) <op> <rhs>`
  - Boolean: `<field> == true` / `false`, `!=`
  - Enum: `<field> == <variant>`, `!=`, `<field> in <v1>,<v2>`, `<field> not_in <v1>,<v2>`

The parser rejects anything it would otherwise have to guess about, because a silently dropped token shrinks the domain that `diff` claims to cover: stray tokens after a field declaration, duplicate field names, enum variants, rule ids, `pipeline` or `default` headers, duplicate unit multipliers, and `scaled(...)` multipliers whose products over the value field's range overflow `i128`. Enum lists may use `a,b` or `a, b`.

## Verification

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
RUSTFLAGS="-D warnings" cargo test --release
python3 scripts/crosscheck.py
```

To replay the Lean 4 soundness proofs:

```sh
MATHLIB_DIR=/path/to/mathlib4 ./lean/replay.sh
```

## License

Apache-2.0. See [`LICENSE`](https://github.com/san-digital/seam-atlas/blob/main/LICENSE).
