# Changelog

Notable changes to `seam-atlas`. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[semantic versioning](https://semver.org/).

## Unreleased

### Changed

- CLI errors (bad arguments, unreadable file, parse error, policies with different
  fields) now exit with status 2. Status 1 now only means `diff --fail-on-diff` found a
  change, so CI can tell the two apart. Scripts that treated any non-zero status as
  "changed" keep working; scripts that checked for exactly 1 on errors need updating.
- Text output of `diff` shows the old and new rule ids, and both commands use plainer
  wording. The JSON output is unchanged.
- The example dosing policies use the pipeline name `dose_triage` and are marked as
  illustrative, not clinical guidance.

### Added

- `--version` and per-command `--help`, with exit statuses in the help text.
- The schema mismatch error names the first field that differs.

### Fixed

- The parser rejects empty `in`/`not_in` lists, duplicate `severity=` attributes, and
  field names that are keywords or contain operator characters. Each of these used to
  parse into something other than what was written.
- A closed stdout pipe (for example `| head`) no longer makes the CLI panic.

## 0.1.1 - 2026-11-21

### Fixed

- `field role enum viewer, admin` declared only `viewer`: enum lists now take the rest of
  the line, and stray tokens in field declarations are errors.
- Duplicate fields, enum variants, rule ids, `pipeline`/`default` headers and unit
  multipliers are rejected.
- `scaled(...)` multipliers whose products could overflow `i128` are rejected. Release
  builds used to wrap and misclassify inputs.
- JSON output escapes all control characters.
- Unknown CLI options are rejected instead of being read as file paths.

### Added

- CI replays the Lean proofs and checks their axioms.
- A randomized test compares partitions, diffs and cliffs with brute-force enumeration.

## 0.1.0 - 2026-11-21

- First release: the `diff` and `cliffs` commands and the library API.
