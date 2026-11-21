# Contributing

Bug reports, questions and pull requests are welcome. For anything larger than a small
fix, please open an issue first so we can agree on the approach.

## Build and test

You need a Rust toolchain (1.81 or later) and Python 3 for the cross-check script.

```sh
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test                  # debug build, also runs the debug_assert checks
cargo test --release
python3 scripts/crosscheck.py
```

CI runs the same commands, plus `cargo doc`, `cargo package` and a build on Rust 1.81.

`tests/hygiene.rs` also checks the text of the repository: no runtime dependencies, no
`unsafe`, no typographic quotes or dashes, and a short list of banned words. If it
fails on your change, the message names the file and line.

If you change the CLI output or the example policies, update the README: `tests/readme.rs`
checks that its quick-start output matches the CLI.

## Lean proofs

The proofs in `lean/SeamAtlas/` are replayed by the `lean` CI job. To run them locally,
install [elan](https://github.com/leanprover/elan), then:

```sh
git clone --depth 1 --branch v4.33.1 https://github.com/leanprover-community/mathlib4
(cd mathlib4 && lake exe cache get)
MATHLIB_DIR=$PWD/mathlib4 ./lean/replay.sh
```

The script fails if a theorem uses `sorry`, `native_decide` or any axiom other than
`propext`, `Classical.choice` and `Quot.sound`. See [`PROOF.md`](PROOF.md) for what the
proofs do and do not cover.

## Pull requests

- Keep changes focused, and add a test for any bug fix.
- Add a line under "Unreleased" in [`CHANGELOG.md`](CHANGELOG.md) for user-visible changes.
- By contributing you agree that your work is licensed under the Apache License 2.0.
