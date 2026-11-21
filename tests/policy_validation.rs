use std::fs;
use std::path::PathBuf;
use std::process::Command;

use seam_atlas::{cliffs_to_json, diff_pipelines, find_cliffs, parse_policy, Domain};

fn parse_err(src: &str) -> String {
    match parse_policy(src) {
        Ok(_) => panic!("expected a parse error for:\n{src}"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn enum_lists_with_spaces_after_commas_declare_every_variant() {
    // Before the fix, `viewer, admin` silently declared only `viewer`, so this diff
    // reported "EQUIVALENT across all 2 input states" and never modelled admins.
    let old = parse_policy(
        "pipeline access
default Deny severity=2
field role enum viewer, admin
field env enum dev, prod
rule allow_viewer -> Allow severity=0:
  role == viewer
",
    )
    .unwrap();
    let new = parse_policy(
        "pipeline access
default Deny severity=2
field role enum viewer, admin
field env enum dev, prod
rule allow_all -> Allow severity=0:
",
    )
    .unwrap();

    assert_eq!(
        old.schema.fields[0].domain,
        Domain::Enum {
            variants: vec!["viewer".to_owned(), "admin".to_owned()]
        }
    );
    let diff = diff_pipelines(&old, &new).unwrap();
    assert_eq!(diff.total_volume, 4);
    assert_eq!(diff.changed_volume, 2);
    assert!(!diff.is_equivalent);
}

#[test]
fn field_declarations_reject_stray_tokens() {
    let base = "pipeline p\ndefault A severity=0\n";
    let msg = parse_err(&format!("{base}field env enum dev,prod extra\n"));
    assert!(msg.contains("invalid enum variant 'prod extra'"), "{msg}");

    let msg = parse_err(&format!("{base}field x int 0..10 junk\n"));
    assert!(msg.contains("unexpected token 'junk'"), "{msg}");

    let msg = parse_err(&format!("{base}field flag bool yes\n"));
    assert!(msg.contains("unexpected token 'yes'"), "{msg}");
}

#[test]
fn duplicate_declarations_are_rejected() {
    let base = "pipeline p\ndefault A severity=0\n";

    let msg = parse_err(&format!("{base}field x int 0..3\nfield x int 0..3\n"));
    assert!(msg.contains("duplicate field 'x'"), "{msg}");

    let msg = parse_err(&format!("{base}field m enum a,b,a\n"));
    assert!(msg.contains("duplicate enum variant 'a'"), "{msg}");

    let msg = parse_err(&format!(
        "{base}field x int 0..3\nrule r1 -> B severity=1:\n  x >= 2\nrule r1 -> C severity=2:\n  x == 0\n"
    ));
    assert!(msg.contains("duplicate rule id 'r1'"), "{msg}");

    let msg = parse_err(&format!("{base}pipeline q\nfield x int 0..3\n"));
    assert!(msg.contains("duplicate 'pipeline' header"), "{msg}");

    let msg = parse_err(&format!("{base}default B severity=1\nfield x int 0..3\n"));
    assert!(msg.contains("duplicate 'default' outcome"), "{msg}");
}

#[test]
fn scaled_multipliers_must_fit_in_i128_over_the_value_domain() {
    let header = "pipeline p\ndefault Low severity=0\nfield unit enum mg,g\n";

    // 10 * (2^127 - 1) overflows i128: release builds used to wrap and misclassify `dose=5, unit=g`.
    let msg = parse_err(&format!(
        "{header}field dose int 0..10\nrule big -> High severity=3:\n  scaled(dose, unit, mg=1, g={}) >= 5\n",
        i128::MAX
    ));
    assert!(msg.contains("overflows 128-bit arithmetic"), "{msg}");

    // The same multiplier is fine when every product still fits.
    let ok = parse_policy(&format!(
        "{header}field dose int 0..1\nrule big -> High severity=3:\n  scaled(dose, unit, mg=1, g={}) >= 5\n",
        i128::MAX
    ))
    .unwrap();
    assert!(find_cliffs(&ok).iter().any(|c| c.field_name == "unit"));

    let msg = parse_err(&format!(
        "{header}field dose int 0..10\nrule big -> High severity=3:\n  scaled(dose, unit, mg=1, mg=2, g=1000) >= 5\n"
    ));
    assert!(msg.contains("duplicate multiplier for unit 'mg'"), "{msg}");
}

#[test]
fn json_output_escapes_control_characters() {
    let pipeline = parse_policy(
        "pipeline ctl\ndefault Lo\u{1}w severity=0\nfield x int 0..1\nrule hi -> High severity=1:\n  x == 1\n",
    )
    .unwrap();
    let json = cliffs_to_json(&pipeline.schema, &find_cliffs(&pipeline));
    assert!(json.contains("Lo\\u0001w"), "{json}");
    assert!(!json.contains('\u{1}'));
}

#[test]
fn cli_rejects_unknown_options() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let v1 = root.join("policies/iam_v1.policy");
    let v2 = root.join("policies/iam_v2.policy");
    assert!(fs::metadata(&v1).is_ok() && fs::metadata(&v2).is_ok());

    let status = Command::new(env!("CARGO_BIN_EXE_seam-atlas"))
        .arg("diff")
        .arg("--fail-on-dif")
        .arg(&v1)
        .arg(&v2)
        .output()
        .unwrap();
    assert_eq!(status.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&status.stderr).contains("unknown option '--fail-on-dif'"));

    let status = Command::new(env!("CARGO_BIN_EXE_seam-atlas"))
        .args(["cliffs", "--jsno"])
        .arg(&v1)
        .output()
        .unwrap();
    assert_eq!(status.status.code(), Some(2));
}

#[test]
fn ambiguous_guards_and_attributes_are_rejected() {
    let base = "pipeline p\ndefault A severity=0\nfield role enum a,b\n";

    // An empty list used to parse as "never matches" (or "always matches" for not_in).
    let msg = parse_err(&format!("{base}rule r -> B severity=1:\n  role in ,\n"));
    assert!(msg.contains("at least one enum variant"), "{msg}");

    let msg = parse_err(&format!("{base}rule r -> B severity=1 severity=2:\n"));
    assert!(msg.contains("duplicate 'severity='"), "{msg}");

    // Guards on these names would be read as headers or split at the wrong operator.
    let msg = parse_err("pipeline p\ndefault A severity=0\nfield rule bool\n");
    assert!(msg.contains("'rule' is a keyword"), "{msg}");
    let msg = parse_err("pipeline p\ndefault A severity=0\nfield a<b int 0..3\n");
    assert!(msg.contains("must not contain '<'"), "{msg}");
}

#[test]
fn int_ranges_are_inclusive_at_both_ends() {
    let p = parse_policy(
        "pipeline p\ndefault Out severity=0\nfield x int 0..10\nrule r -> In severity=1:\n  x in 2..4\n",
    )
    .unwrap();
    assert_eq!(p.schema.total_volume(), 11);
    let inside: Vec<i64> = (0..=10)
        .filter(|&x| p.eval(&[seam_atlas::Value::Int(x)]).0.label == "In")
        .collect();
    assert_eq!(inside, vec![2, 3, 4]);
}

fn run_cli(args: &[&str]) -> (Option<i32>, String, String) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out = Command::new(env!("CARGO_BIN_EXE_seam-atlas"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn cli_exit_codes_separate_changed_outcomes_from_errors() {
    let (v1, v2) = ("policies/iam_v1.policy", "policies/iam_v2.policy");

    // A diff without --fail-on-diff is a successful run.
    let (code, stdout, _) = run_cli(&["diff", v1, v2]);
    assert_eq!(code, Some(0));
    assert!(
        stdout.starts_with("CHANGED: 314572803 of 22649241816"),
        "{stdout}"
    );
    assert!(stdout.contains("rule require_prod_mfa"), "{stdout}");

    let (code, _, _) = run_cli(&["diff", "--fail-on-diff", v1, v2]);
    assert_eq!(code, Some(1));
    let (code, stdout, _) = run_cli(&["diff", "--fail-on-diff", v1, v1]);
    assert_eq!(code, Some(0));
    assert!(stdout.starts_with("EQUIVALENT"), "{stdout}");

    // Errors use 2, so `--fail-on-diff` in CI can tell "changed" from "broken".
    let (code, _, stderr) = run_cli(&["diff", "--fail-on-diff", v1, "missing.policy"]);
    assert_eq!(code, Some(2));
    assert!(stderr.contains("cannot read 'missing.policy'"), "{stderr}");
    let (code, _, stderr) = run_cli(&["diff", v1, "policies/clinical_v1.policy"]);
    assert_eq!(code, Some(2));
    assert!(stderr.contains("declare different fields"), "{stderr}");
    let (code, _, stderr) = run_cli(&["cliffs", "Cargo.toml"]);
    assert_eq!(code, Some(2));
    assert!(stderr.starts_with("error: Cargo.toml:line "), "{stderr}");
}

#[test]
fn cli_help_and_version() {
    for args in [&["--help"][..], &["diff", "--help"], &["cliffs", "-h"]] {
        let (code, stdout, _) = run_cli(args);
        assert_eq!(code, Some(0), "{args:?}");
        assert!(stdout.contains("exit status:"), "{stdout}");
    }
    let (code, stdout, _) = run_cli(&["--version"]);
    assert_eq!(code, Some(0));
    assert_eq!(
        stdout.trim(),
        format!("seam-atlas {}", env!("CARGO_PKG_VERSION"))
    );
    let (code, _, _) = run_cli(&[]);
    assert_eq!(code, Some(2));
}
