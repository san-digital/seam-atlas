# seam-atlas

[![crates.io](https://img.shields.io/crates/v/seam-atlas.svg)](https://crates.io/crates/seam-atlas)
[![docs.rs](https://docs.rs/seam-atlas/badge.svg)](https://docs.rs/seam-atlas)
[![CI](https://github.com/san-digital/seam-atlas/actions/workflows/ci.yml/badge.svg)](https://github.com/san-digital/seam-atlas/actions/workflows/ci.yml)
[![License](https://img.shields.io/crates/l/seam-atlas.svg)](https://github.com/san-digital/seam-atlas/blob/main/LICENSE)

Point `seam-atlas` at two versions of a rules file and it tells you exactly which inputs
change outcome. Point it at one version and it lists the places where a one-step change
to a single field (3999 to 4000, `mg` to `g`, `false` to `true`) flips the result.

It is for anyone who maintains first-match rule lists: access policies, feature gates,
eligibility or pricing rules, rate limits, safety checks. It checks every possible input,
not a sample, and you do not need to write a separate spec: the old version of the rules
is the reference for the new one.

You describe the rules in a small text format ([policy syntax](#policy-syntax)).
`seam-atlas` reads that file, not your application code.

## Install

```sh
cargo install seam-atlas
```

To use it as a library, run `cargo add seam-atlas`. It has no dependencies and needs Rust
1.81 or later.

## Quick start

The example policies live in the repository:

```sh
git clone https://github.com/san-digital/seam-atlas
cd seam-atlas
```

The commands below use the installed `seam-atlas`. To run the code in the clone instead,
use `cargo run --release --` in its place.

Compare two versions of an access policy. In
[`iam_v2.policy`](https://github.com/san-digital/seam-atlas/blob/main/policies/iam_v2.policy)
a refactor moved `allow_auditor_read` above `require_prod_mfa`:

```sh
seam-atlas diff policies/iam_v1.policy policies/iam_v2.policy
```

```text
CHANGED: 314572803 of 22649241816 inputs get a different outcome, in 1 group(s):
  Deny (sev=3, rule require_prod_mfa) -> Allow (sev=0, rule allow_auditor_read) [inputs=314572803, regions=1] example={role=auditor, env=prod, action=read, mfa=false, ip_zone=corp, payload_bytes=0}
```

All 22,649,241,816 possible inputs were checked, in milliseconds. Exactly one kind of
input changed: auditors reading in prod without MFA, which v1 denied and v2 allows. Add
`--fail-on-diff` to make CI fail when a change that should not alter behavior does.

Now look for cliff edges in a single policy:

```sh
seam-atlas cliffs policies/clinical_v1.policy
```

> The dosing examples are made up to show the tool. The thresholds are invented and
> are not clinical guidance.

```text
pipeline 'dose_triage': 7260726 inputs fall into 9 regions with a fixed outcome
found 7 cliff edges (5 unbuffered, severity jumps by 2 or more):
  [UNBUFFERED] field=dose delta_sev=3: Routine ({age_years=1, dose=3999, unit=mg, hepatic_risk=false}) -> Emergency ({age_years=1, dose=4000, unit=mg, hepatic_risk=false})
  [UNBUFFERED] field=unit delta_sev=3: Routine ({age_years=0, dose=4000, unit=mcg, hepatic_risk=false}) -> Emergency ({age_years=0, dose=4000, unit=mg, hepatic_risk=false})
  [UNBUFFERED] field=age_years delta_sev=2: Escalate ({age_years=0, dose=61, unit=mg, hepatic_risk=false}) -> Routine ({age_years=1, dose=61, unit=mg, hepatic_risk=false})
  ...
```

The first line says that one more mg takes a 1-year-old from Routine straight to
Emergency, with no warning level in between.
[`clinical_v1.policy`](https://github.com/san-digital/seam-atlas/blob/main/policies/clinical_v1.policy)
is written with that gap on purpose;
[`clinical_v2.policy`](https://github.com/san-digital/seam-atlas/blob/main/policies/clinical_v2.policy)
adds Caution and Abstain levels.

## Commands

### `diff <old> <new>`

Compares the two policies on every input. An input has changed when its outcome (label
or severity) differs; an input that keeps its outcome through a different rule counts as
unchanged. Changed inputs are grouped by old rule and new rule, and each group comes with
one example input that you can paste into a test.

Both files must declare the same fields, with the same types, ranges and enum variant
order. Rules, outcomes and the default can differ.

### `cliffs <policy>`

Finds pairs of inputs that differ on one field by one step and get different outcomes. A
step is +1 on an integer, `false` to `true` on a bool, or any change of variant on an
enum. Every such boundary is found, but only one example pair is shown per combination
of field, outcome and rule before, and outcome and rule after.

`[UNBUFFERED]` marks pairs whose severities differ by 2 or more: a single step skips at
least one severity level.

### Options and exit status

| Option | Meaning |
| :--- | :--- |
| `--json` | Print JSON instead of text. Counts are strings, since they can exceed 2^53. |
| `--fail-on-diff` | `diff` only: exit with status 1 if any outcome changed. |
| `-h`, `--help` | Print help. |
| `-V`, `--version` | Print the version. |

Exit status is 0 on success, 1 when `--fail-on-diff` finds a change, and 2 for errors
(bad arguments, unreadable file, parse error, or policies with different fields).

## Policy syntax

```text
# Comments start with '#'.
pipeline rate_gate
default Allow severity=0

field req_per_min int 0..10000      # 0 to 10000 inclusive: 10001 values
field tier enum free,pro
field verified bool

rule hard_block -> Block severity=3:
  tier == free
  req_per_min >= 600

rule unverified_review -> Review severity=1:
  verified == false
  req_per_min in 100..599
```

- `pipeline <name>` and `default <Label> severity=<n>` appear once each.
- Severity is an integer from 0 to 255 and defaults to 0 if left out. Higher means more
  serious; `cliffs` uses it to measure jumps, and `diff` treats an outcome with the same
  label but a different severity as a change.
- Fields are declared before any rule:
  - `field <name> int <min>..<max>`: an integer from `min` to `max`, both included.
  - `field <name> bool`
  - `field <name> enum <a>,<b>,...`: spaces after the commas are fine.
- `rule <id> -> <Label> severity=<n>:` starts a rule. The guard lines below it must all
  hold for the rule to match. A rule with no guards always matches.
- Rules are tried from top to bottom and the first match wins. If none match, the
  default applies.

Guards:

| Field type | Guards |
| :--- | :--- |
| int | `x == 5`, `x != 5`, `x < 5`, `x <= 5`, `x > 5`, `x >= 5`, `x in 10..20` (10 and 20 included) |
| bool | `flag == true`, `flag != false` |
| enum | `env == prod`, `env != prod`, `env in dev,staging`, `env not_in prod` |
| int with a unit | `scaled(dose, unit, mcg=1, mg=1000, g=1000000) >= 4000000` |

`scaled(value, unit, ...)` multiplies an int field by a factor chosen by an enum field,
then compares the product. Every variant of the unit field needs a positive integer
factor.

The parser rejects anything it would otherwise have to guess about, because a dropped
token would shrink the set of inputs that `diff` claims to cover. That includes stray
tokens, duplicate names, ids, headers, severities and unit factors, empty `in` lists,
field names that are keywords or contain operator characters, and `scaled(...)` factors
whose products could overflow 128-bit integers.

## Library use

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

// For free-tier users, 599 -> 600 requests per minute jumps from Allow (0) to Block (3).
let cliffs = find_cliffs(&v1);
assert!(cliffs.iter().any(|c| c.field_name == "req_per_min" && c.unbuffered_seam));

// Rewriting `>= 600` as `> 599` and reordering the guards changes nothing,
// and all 10001 * 2 * 2 = 40,004 inputs are checked to confirm it.
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

See [docs.rs](https://docs.rs/seam-atlas) for the full API.

## Limits

`seam-atlas` is exact for the rule shapes above and nothing else:

- Each guard tests one field against a constant (plus a unit field for `scaled`). There
  are no comparisons between fields (`a < b`), no arithmetic across fields, no strings or
  regexes, no floats (use integers in the smallest unit, such as cents), and no state or
  time.
- A rule is an AND of guards. For OR, write several rules with the same outcome.
- The tool checks the policy file. If your application implements the rules separately,
  nothing here checks that the two agree.
- Run time depends on how many regions the rules split the inputs into, not on how many
  inputs there are. That is small for typical policies but can grow exponentially with
  many overlapping rules, and `cliffs` is quadratic in the number of regions.
- Input counts saturate at `u128::MAX` (about 3.4e38), which only happens for schemas such
  as three fields that each span the whole `i64` range.

## How it is checked

`seam-atlas` splits the inputs into regions where every guard is constant and evaluates
one example per region, so its answers are computed, not sampled. The Rust code is tested,
not formally verified: a test compares partitions, diffs and cliffs against brute-force
enumeration on 10,000 random small policies, and
[`scripts/crosscheck.py`](https://github.com/san-digital/seam-atlas/blob/main/scripts/crosscheck.py)
re-checks the CLI output with an independent Python evaluator. Separate Lean 4 proofs
show that the method is sound and complete in the abstract, given regions where every
guard is constant. They do not cover the Rust code itself.
[`PROOF.md`](https://github.com/san-digital/seam-atlas/blob/main/PROOF.md) has the details.

## Contributing and security

See [`CONTRIBUTING.md`](https://github.com/san-digital/seam-atlas/blob/main/CONTRIBUTING.md)
for how to build and test, and
[`SECURITY.md`](https://github.com/san-digital/seam-atlas/blob/main/SECURITY.md) to report a
vulnerability privately. Changes are listed in
[`CHANGELOG.md`](https://github.com/san-digital/seam-atlas/blob/main/CHANGELOG.md).

## License

Apache-2.0. See [`LICENSE`](https://github.com/san-digital/seam-atlas/blob/main/LICENSE).
