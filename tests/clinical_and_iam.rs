use std::fs;
use std::path::PathBuf;

use seam_atlas::{
    cliffs_to_json, compile_partition, diff_pipelines, diff_to_json, find_cliffs, parse_policy,
    Value,
};

#[test]
fn clinical_guardrail_discovers_unit_flip_and_subcutoff_cliffs_and_verifies_v2_fix() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let v1_src = fs::read_to_string(root.join("policies/clinical_v1.policy")).unwrap();
    let v2_src = fs::read_to_string(root.join("policies/clinical_v2.policy")).unwrap();

    let v1 = parse_policy(&v1_src).unwrap();
    let v2 = parse_policy(&v2_src).unwrap();

    // Total volume = 121 * 10,001 * 3 * 2 = 7,260,726 states.
    let part_v1 = compile_partition(&v1);
    assert_eq!(part_v1.total_volume(), 7_260_726);
    assert_eq!(part_v1.total_volume(), v1.schema.total_volume());

    let cliffs_v1 = find_cliffs(&v1);
    assert!(!cliffs_v1.is_empty());

    // Must detect an unbuffered severity-3 cliff on `dose` (3999 mg -> 4000 mg) and on `unit` (mg -> g).
    assert!(cliffs_v1
        .iter()
        .any(|c| c.field_name == "dose" && c.severity_delta == 3 && c.unbuffered_seam));
    assert!(cliffs_v1
        .iter()
        .any(|c| c.field_name == "unit" && c.severity_delta == 3 && c.unbuffered_seam));

    // Verify every emitted cliff pair directly against `v1.eval`.
    for c in &cliffs_v1 {
        assert_eq!(v1.eval(&c.input_a).0, &c.outcome_a);
        assert_eq!(v1.eval(&c.input_b).0, &c.outcome_b);
        let diffs = c
            .input_a
            .iter()
            .zip(c.input_b.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(diffs, 1, "cliff witness must differ in only 1 coordinate");
    }

    // Behavioral diff from v1 to v2 must never weaken `Emergency` (severity 3) anywhere.
    let diff = diff_pipelines(&v1, &v2).unwrap();
    assert!(!diff.is_equivalent);
    assert_eq!(diff.total_volume, 7_260_726);
    for cls in &diff.classes {
        assert!(
            cls.new_outcome.severity >= cls.old_outcome.severity,
            "unexpected severity downgrade: {:?} -> {:?}",
            cls.old_outcome,
            cls.new_outcome
        );
        assert_eq!(v1.eval(&cls.witness).0, &cls.old_outcome);
        assert_eq!(v2.eval(&cls.witness).0, &cls.new_outcome);
    }

    // JSON serialization smoke check.
    let json_c = cliffs_to_json(&v1.schema, &cliffs_v1);
    assert!(json_c.contains("\"cliff_edges\""));
    let json_d = diff_to_json(&v1.schema, &diff);
    assert!(json_d.contains("\"changed_volume\""));
}

#[test]
fn cloud_iam_catches_auditor_prod_mfa_bypass_across_22_billion_states() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let iam1_src = fs::read_to_string(root.join("policies/iam_v1.policy")).unwrap();
    let iam2_src = fs::read_to_string(root.join("policies/iam_v2.policy")).unwrap();

    let iam1 = parse_policy(&iam1_src).unwrap();
    let iam2 = parse_policy(&iam2_src).unwrap();

    // 4 * 3 * 3 * 2 * 3 * 104,857,601 = 22,649,241,816 states.
    assert_eq!(iam1.schema.total_volume(), 22_649_241_816);

    let report = diff_pipelines(&iam1, &iam2).unwrap();
    assert!(!report.is_equivalent);
    assert_eq!(report.total_volume, 22_649_241_816);

    // Exactly auditor (1) * prod (1) * read (1) * mfa=false (1) * ip_zone (3) * payload (104,857,601)
    // = 314,572,803 states flipped from Deny (require_prod_mfa) to Allow (allow_auditor_read).
    assert_eq!(report.changed_volume, 3 * 104_857_601);
    assert_eq!(report.classes.len(), 1);
    let cls = &report.classes[0];
    assert_eq!(cls.old_outcome.label, "Deny");
    assert_eq!(cls.old_rule.as_deref(), Some("require_prod_mfa"));
    assert_eq!(cls.new_outcome.label, "Allow");
    assert_eq!(cls.new_rule.as_deref(), Some("allow_auditor_read"));
    assert_eq!(cls.witness[3], Value::Bool(false)); // mfa == false
}
