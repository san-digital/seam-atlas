//! Keeps the README honest: its policy example must parse and its quick-start output
//! must match what the CLI prints.

use std::path::PathBuf;
use std::process::Command;

use seam_atlas::parse_policy;

fn readme() -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(root.join("README.md")).unwrap()
}

/// Returns the first fenced block with the given info string after `heading`.
fn block_after(doc: &str, heading: &str, lang: &str) -> String {
    let start = doc.find(heading).expect("heading present");
    let fence = format!("```{lang}\n");
    let body_start = start + doc[start..].find(&fence).expect("block present") + fence.len();
    let len = doc[body_start..].find("```").unwrap();
    doc[body_start..body_start + len].to_owned()
}

fn cli(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_seam-atlas"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "{args:?}");
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn policy_syntax_example_parses() {
    let src = block_after(&readme(), "## Policy syntax", "text");
    let p = parse_policy(&src).unwrap();
    assert_eq!(p.schema.total_volume(), 10_001 * 2 * 2);
    assert_eq!(p.rules.len(), 2);
}

#[test]
fn quick_start_output_matches_the_cli() {
    let doc = readme();
    let quick = &doc[doc.find("## Quick start").unwrap()..];

    let shown = block_after(quick, "seam-atlas diff policies/iam_v1", "text");
    let actual = cli(&["diff", "policies/iam_v1.policy", "policies/iam_v2.policy"]);
    assert_eq!(shown, actual);

    // The README shortens the cliffs output with a trailing "...".
    let shown = block_after(quick, "seam-atlas cliffs policies/clinical_v1", "text");
    let actual = cli(&["cliffs", "policies/clinical_v1.policy"]);
    let prefix = shown.strip_suffix("  ...\n").expect("shortened output");
    assert!(
        actual.starts_with(prefix),
        "README:\n{shown}\nCLI:\n{actual}"
    );
}
