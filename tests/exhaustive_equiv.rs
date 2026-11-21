use seam_atlas::{compile_partition, diff_pipelines, find_cliffs, parse_policy, Value};

#[test]
fn partition_and_diff_match_brute_force_grid_enumeration() {
    let policy_a = "\
pipeline grid_a
default Pass severity=0

field x int -4..6
field unit enum u1,u10
field flag bool
field mode enum alpha,beta,gamma

rule high_scaled -> Block severity=3:
  scaled(x, unit, u1=1, u10=10) >= 25
  flag == true

rule mid_range -> Warn severity=1:
  x in 1..4
  mode not_in gamma

rule neg_exact -> Review severity=2:
  x == -2
  mode == gamma
";

    let policy_b = "\
pipeline grid_b
default Pass severity=0

field x int -4..6
field unit enum u1,u10
field flag bool
field mode enum alpha,beta,gamma

rule high_scaled -> Block severity=3:
  scaled(x, unit, u1=1, u10=10) >= 20
  flag == true

rule neg_exact -> Review severity=2:
  x == -2
  mode == gamma

rule mid_range -> Warn severity=1:
  x >= 1
  x <= 4
  mode in alpha,beta
";

    let pa = parse_policy(policy_a).unwrap();
    let pb = parse_policy(policy_b).unwrap();

    // 1. Check total volume: 11 * 2 * 2 * 3 = 132 points.
    let part_a = compile_partition(&pa);
    assert_eq!(part_a.total_volume(), 132);

    // 2. Brute-force check every point in the 132-point Cartesian grid.
    let mut brute_changed = 0u128;
    for x in -4..=6 {
        for u in 0..2 {
            for flag in [false, true] {
                for mode in 0..3 {
                    let pt = vec![
                        Value::Int(x),
                        Value::Enum(u),
                        Value::Bool(flag),
                        Value::Enum(mode),
                    ];
                    let oa = pa.eval(&pt).0;
                    let ob = pb.eval(&pt).0;
                    if oa != ob {
                        brute_changed += 1;
                    }
                }
            }
        }
    }

    let diff = diff_pipelines(&pa, &pb).unwrap();
    assert_eq!(diff.total_volume, 132);
    assert_eq!(diff.changed_volume, brute_changed);
    assert!(brute_changed > 0);

    // 3. Reordering non-overlapping rules produces an exact equivalence proof (`changed_volume == 0`).
    let pb_equiv = "\
pipeline grid_b_equiv
default Pass severity=0

field x int -4..6
field unit enum u1,u10
field flag bool
field mode enum alpha,beta,gamma

rule neg_exact -> Review severity=2:
  x == -2
  mode == gamma

rule high_scaled -> Block severity=3:
  scaled(x, unit, u1=1, u10=10) >= 20
  flag == true

rule mid_range -> Warn severity=1:
  x in 1..4
  mode != gamma
";
    let pb2 = parse_policy(pb_equiv).unwrap();
    let eq_diff = diff_pipelines(&pb, &pb2).unwrap();
    assert!(eq_diff.is_equivalent);
    assert_eq!(eq_diff.changed_volume, 0);

    // 4. Every cliff edge in `pa` is a genuine 1-step boundary flip.
    let cliffs = find_cliffs(&pa);
    assert!(!cliffs.is_empty());
    for c in cliffs {
        assert_ne!(pa.eval(&c.input_a).0, pa.eval(&c.input_b).0);
    }
}
