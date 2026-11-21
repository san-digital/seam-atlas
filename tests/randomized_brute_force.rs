//! Randomized cross-check of `compile_partition`, `diff_pipelines`, and `find_cliffs`
//! against exhaustive enumeration.
//!
//! The Lean modules prove that per-cell witness checks are sound and complete once the
//! cells form a predicate-constant cover. Whether the Rust refinement (`eval_pred_on_box`,
//! `split_box_for_pred`) really produces such a cover is checked here: every generated
//! policy has a small domain, so each claim can be compared point by point.

use std::collections::BTreeSet;

use seam_atlas::{
    compile_partition, diff_pipelines, find_cliffs, parse_policy, CoordSpan, Domain, LeafRegion,
    Pipeline, Schema, Value,
};

const ITERATIONS: u64 = 10_000;

/// Deterministic xorshift64 generator, so failures reproduce from the iteration number.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }
}

#[derive(Clone)]
enum Field {
    Int(i64, i64),
    Bool,
    Enum(usize),
}

const OPS: [&str; 6] = [">=", "<=", "==", "!=", ">", "<"];
const OUTCOMES: [(&str, u8); 5] = [("A", 0), ("B", 1), ("C", 2), ("D", 3), ("A", 2)];

fn gen_schema(rng: &mut Rng) -> Vec<Field> {
    (0..1 + rng.below(4))
        .map(|_| match rng.below(3) {
            0 => {
                let lo = rng.range(-6, 6);
                Field::Int(lo, rng.range(lo, (lo + 8).min(9)))
            }
            1 => Field::Bool,
            _ => Field::Enum(1 + rng.below(4) as usize),
        })
        .collect()
}

fn gen_pred(rng: &mut Rng, fields: &[Field]) -> String {
    let i = rng.below(fields.len() as u64) as usize;
    match fields[i] {
        Field::Int(lo, hi) => {
            if rng.below(4) == 0 {
                if let Some(u) = fields.iter().position(|f| matches!(f, Field::Enum(_))) {
                    let Field::Enum(k) = fields[u] else {
                        unreachable!()
                    };
                    let maps: Vec<String> = (0..k)
                        .map(|j| format!("v{j}={}", 1 + rng.below(12)))
                        .collect();
                    let op = OPS[rng.below(6) as usize];
                    let rhs = rng.range(-80, 80);
                    return format!("scaled(f{i}, f{u}, {}) {op} {rhs}", maps.join(", "));
                }
            }
            if rng.below(5) == 0 {
                let a = rng.range(lo - 2, hi + 2);
                return format!("f{i} in {a}..{}", rng.range(a, hi + 3));
            }
            let op = OPS[rng.below(6) as usize];
            format!("f{i} {op} {}", rng.range(lo - 2, hi + 2))
        }
        Field::Bool => {
            let op = if rng.below(2) == 0 { "==" } else { "!=" };
            format!("f{i} {op} {}", rng.below(2) == 0)
        }
        Field::Enum(k) => match rng.below(4) {
            0 => format!("f{i} == v{}", rng.below(k as u64)),
            1 => format!("f{i} != v{}", rng.below(k as u64)),
            kind => {
                let mut picked: Vec<String> = (0..k)
                    .filter(|_| rng.below(2) == 0)
                    .map(|j| format!("v{j}"))
                    .collect();
                if picked.is_empty() {
                    picked.push(format!("v{}", rng.below(k as u64)));
                }
                let op = if kind == 2 { "in" } else { "not_in" };
                format!("f{i} {op} {}", picked.join(","))
            }
        },
    }
}

fn gen_policy(rng: &mut Rng, fields: &[Field], name: &str) -> String {
    let (label, sev) = OUTCOMES[rng.below(5) as usize];
    let mut src = format!("pipeline {name}\ndefault {label} severity={sev}\n");
    for (i, f) in fields.iter().enumerate() {
        match f {
            Field::Int(lo, hi) => src += &format!("field f{i} int {lo}..{hi}\n"),
            Field::Bool => src += &format!("field f{i} bool\n"),
            Field::Enum(k) => {
                let names: Vec<String> = (0..*k).map(|j| format!("v{j}")).collect();
                src += &format!("field f{i} enum {}\n", names.join(","));
            }
        }
    }
    for r in 0..rng.below(6) {
        let (label, sev) = OUTCOMES[rng.below(5) as usize];
        src += &format!("rule r{r} -> {label} severity={sev}:\n");
        for _ in 0..rng.below(4) {
            src += &format!("  {}\n", gen_pred(rng, fields));
        }
    }
    src
}

fn all_points(schema: &Schema) -> Vec<Vec<Value>> {
    let mut points = vec![Vec::new()];
    for f in &schema.fields {
        let values: Vec<Value> = match &f.domain {
            Domain::IntRange { min, max } => (*min..=*max).map(Value::Int).collect(),
            Domain::Bool => vec![Value::Bool(false), Value::Bool(true)],
            Domain::Enum { variants } => (0..variants.len()).map(Value::Enum).collect(),
        };
        points = points
            .iter()
            .flat_map(|p| {
                values.iter().map(move |v| {
                    let mut q = p.clone();
                    q.push(v.clone());
                    q
                })
            })
            .collect();
    }
    points
}

fn contains(region: &LeafRegion, x: &[Value]) -> bool {
    region.spans.iter().zip(x).all(|(span, v)| match (span, v) {
        (CoordSpan::Int { lo, hi }, Value::Int(a)) => lo <= a && a <= hi,
        (CoordSpan::BoolMask(m), Value::Bool(b)) => (m >> u8::from(*b)) & 1 == 1,
        (CoordSpan::EnumSet(vs), Value::Enum(e)) => vs.contains(e),
        _ => false,
    })
}

/// Outcome label, severity, and matched rule id at `x`.
type Side = (String, u8, Option<String>);

fn side(p: &Pipeline, x: &[Value]) -> Side {
    let (outcome, rule) = p.eval(x);
    (
        outcome.label.clone(),
        outcome.severity,
        rule.map(str::to_owned),
    )
}

fn same_outcome(a: &Side, b: &Side) -> bool {
    a.0 == b.0 && a.1 == b.1
}

fn check_partition(p: &Pipeline, points: &[Vec<Value>]) -> Result<(), String> {
    let part = compile_partition(p);
    if part.total_volume() != points.len() as u128 {
        return Err(format!(
            "partition volume {} != {}",
            part.total_volume(),
            points.len()
        ));
    }
    for x in points {
        let hits: Vec<&LeafRegion> = part.regions.iter().filter(|r| contains(r, x)).collect();
        if hits.len() != 1 {
            return Err(format!("{x:?} lies in {} regions", hits.len()));
        }
        let (outcome, rule) = p.eval(x);
        if &hits[0].outcome != outcome || hits[0].matched_rule.as_deref() != rule {
            return Err(format!("region outcome differs from eval at {x:?}"));
        }
    }
    Ok(())
}

fn check_diff(old: &Pipeline, new: &Pipeline, points: &[Vec<Value>]) -> Result<(), String> {
    let report = diff_pipelines(old, new).ok_or("schemas unexpectedly differ")?;
    let mut changed = 0u128;
    let mut want = BTreeSet::new();
    for x in points {
        let (a, b) = (side(old, x), side(new, x));
        if !same_outcome(&a, &b) {
            changed += 1;
            want.insert((a, b));
        }
    }
    let have: BTreeSet<(Side, Side)> = report
        .classes
        .iter()
        .map(|c| {
            (
                (
                    c.old_outcome.label.clone(),
                    c.old_outcome.severity,
                    c.old_rule.clone(),
                ),
                (
                    c.new_outcome.label.clone(),
                    c.new_outcome.severity,
                    c.new_rule.clone(),
                ),
            )
        })
        .collect();
    if report.total_volume != points.len() as u128
        || report.changed_volume != changed
        || report.is_equivalent != (changed == 0)
        || have != want
    {
        return Err(format!(
            "diff reported {} changed in classes {have:?}, enumeration found {changed} in {want:?}",
            report.changed_volume
        ));
    }
    for c in &report.classes {
        let (a, b) = (side(old, &c.witness), side(new, &c.witness));
        if a.0 != c.old_outcome.label || b.0 != c.new_outcome.label {
            return Err(format!("diff witness {:?} does not reproduce", c.witness));
        }
    }
    Ok(())
}

fn check_cliffs(p: &Pipeline, points: &[Vec<Value>]) -> Result<(), String> {
    // Enum swaps are symmetric, so their orientation (which side is `a`) is normalized.
    let key = |k: usize, a: Side, b: Side| {
        if matches!(p.schema.fields[k].domain, Domain::Enum { .. }) && b < a {
            (k, b, a)
        } else {
            (k, a, b)
        }
    };

    let mut want = BTreeSet::new();
    for x in points {
        for (k, f) in p.schema.fields.iter().enumerate() {
            let steps: Vec<Value> = match (&f.domain, &x[k]) {
                (Domain::IntRange { max, .. }, Value::Int(v)) if v < max => {
                    vec![Value::Int(v + 1)]
                }
                (Domain::Bool, Value::Bool(false)) => vec![Value::Bool(true)],
                (Domain::Enum { variants }, Value::Enum(e)) => {
                    ((e + 1)..variants.len()).map(Value::Enum).collect()
                }
                _ => Vec::new(),
            };
            for v in steps {
                let mut y = x.clone();
                y[k] = v;
                let (a, b) = (side(p, x), side(p, &y));
                if !same_outcome(&a, &b) {
                    want.insert(key(k, a, b));
                }
            }
        }
    }

    let mut have = BTreeSet::new();
    for c in find_cliffs(p) {
        let differing = c
            .input_a
            .iter()
            .zip(&c.input_b)
            .filter(|(u, v)| u != v)
            .count();
        let (a, b) = (side(p, &c.input_a), side(p, &c.input_b));
        if differing != 1 || same_outcome(&a, &b) {
            return Err(format!("invalid cliff edge {c:?}"));
        }
        have.insert(key(c.field_index, a, b));
    }
    if have != want {
        return Err(format!(
            "cliff classes differ: missing {:?}, spurious {:?}",
            want.difference(&have).collect::<Vec<_>>(),
            have.difference(&want).collect::<Vec<_>>()
        ));
    }
    Ok(())
}

#[test]
fn partition_diff_and_cliffs_match_exhaustive_enumeration_on_random_policies() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for iteration in 0..ITERATIONS {
        let fields = gen_schema(&mut rng);
        let src_old = gen_policy(&mut rng, &fields, "old");
        let src_new = gen_policy(&mut rng, &fields, "new");
        let old = parse_policy(&src_old).unwrap();
        let new = parse_policy(&src_new).unwrap();
        let points = all_points(&old.schema);

        let result = check_partition(&old, &points)
            .and_then(|()| check_diff(&old, &new, &points))
            .and_then(|()| check_cliffs(&old, &points));
        if let Err(msg) = result {
            panic!("iteration {iteration}: {msg}\n--- old\n{src_old}\n--- new\n{src_new}");
        }
    }
}
