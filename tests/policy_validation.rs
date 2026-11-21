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
