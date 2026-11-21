use std::collections::BTreeMap;

use crate::model::{Outcome, Pipeline, Value};
use crate::partition::{compile_partition, CoordSpan, LeafRegion, Partition};

/// Minimal single-coordinate perturbation `(input_a, input_b)` across a decision boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliffEdge {
    pub field_index: usize,
    pub field_name: String,
    pub input_a: Vec<Value>,
    pub outcome_a: Outcome,
    pub rule_a: Option<String>,
    pub input_b: Vec<Value>,
    pub outcome_b: Outcome,
    pub rule_b: Option<String>,
    pub severity_delta: u8,
    /// True when `|severity(outcome_a) - severity(outcome_b)| >= 2` across a 1-step boundary.
    pub unbuffered_seam: bool,
}

/// Discovers canonical minimal-perturbation cliff-edges across all decision boundaries of `pipeline`.
///
/// Two inputs form a cliff-edge when they are identical on `n - 1` coordinates, differ by an
/// atomic step (`|x_k - x'_k| == 1` for integers, `false`/`true` for booleans, or a single variant
/// swap for enums) on coordinate `k`, and produce distinct outcomes.
pub fn find_cliffs(pipeline: &Pipeline) -> Vec<CliffEdge> {
    let part = compile_partition(pipeline);
    find_cliffs_in_partition(pipeline, &part)
}

pub fn find_cliffs_in_partition(pipeline: &Pipeline, part: &Partition) -> Vec<CliffEdge> {
    let n_fields = part.schema.fields.len();
    let n_regs = part.regions.len();

    // Deduplicate by canonical boundary signature `(field, outcome_pair, rule_pair)`
    // while keeping the witness pair nicely ordered.
    type Key = (usize, Outcome, Option<String>, Outcome, Option<String>);
    let mut best: BTreeMap<Key, CliffEdge> = BTreeMap::new();

    for i in 0..n_regs {
        for j in (i + 1)..n_regs {
            let r1 = &part.regions[i];
            let r2 = &part.regions[j];
            if r1.outcome == r2.outcome {
                continue;
            }

            for k in 0..n_fields {
                if let Some((va, vb, swapped)) = touch_witness(&r1.spans[k], &r2.spans[k]) {
                    if let Some(mut base) = shared_projection(r1, r2, k) {
                        let (ra, rb) = if swapped { (r2, r1) } else { (r1, r2) };
                        base[k] = va;
                        let input_a = base.clone();
                        base[k] = vb;
                        let input_b = base;

                        // Direct executable check against the source pipeline.
                        let (eval_oa, eval_ra) = pipeline.eval(&input_a);
                        let (eval_ob, eval_rb) = pipeline.eval(&input_b);
                        debug_assert_eq!(eval_oa, &ra.outcome);
                        debug_assert_eq!(eval_ob, &rb.outcome);
                        debug_assert_eq!(eval_ra, ra.matched_rule.as_deref());
                        debug_assert_eq!(eval_rb, rb.matched_rule.as_deref());

                        let sev_delta = ra.outcome.severity.abs_diff(rb.outcome.severity);
                        let edge = CliffEdge {
                            field_index: k,
                            field_name: part.schema.fields[k].name.clone(),
                            input_a,
                            outcome_a: ra.outcome.clone(),
                            rule_a: ra.matched_rule.clone(),
                            input_b,
                            outcome_b: rb.outcome.clone(),
                            rule_b: rb.matched_rule.clone(),
                            severity_delta: sev_delta,
                            unbuffered_seam: sev_delta >= 2,
                        };
                        let key = (
                            k,
                            edge.outcome_a.clone(),
                            edge.rule_a.clone(),
                            edge.outcome_b.clone(),
                            edge.rule_b.clone(),
                        );
                        best.entry(key).or_insert(edge);
                    }
                }
            }
        }
    }

    let mut cliffs: Vec<CliffEdge> = best.into_values().collect();
    cliffs.sort_by(|a, b| {
        b.severity_delta
            .cmp(&a.severity_delta)
            .then_with(|| a.field_index.cmp(&b.field_index))
            .then_with(|| a.outcome_a.label.cmp(&b.outcome_a.label))
            .then_with(|| a.outcome_b.label.cmp(&b.outcome_b.label))
    });
    cliffs
}

fn shared_projection(r1: &LeafRegion, r2: &LeafRegion, skip_field: usize) -> Option<Vec<Value>> {
    let mut tuple = Vec::with_capacity(r1.spans.len());
    for idx in 0..r1.spans.len() {
        if idx == skip_field {
            tuple.push(Value::Bool(false));
        } else {
            let v = r1.spans[idx].shared_witness(&r2.spans[idx])?;
            tuple.push(v);
        }
    }
    Some(tuple)
}

/// Checks whether `s1` and `s2` touch across a 1-step perturbation along a single coordinate.
/// Returns `(val_left, val_right, swapped)` where `swapped` indicates `val_left` came from `s2`.
fn touch_witness(s1: &CoordSpan, s2: &CoordSpan) -> Option<(Value, Value, bool)> {
    match (s1, s2) {
        (CoordSpan::Int { hi: h1, .. }, CoordSpan::Int { lo: l2, .. })
            if (*h1 as i128) + 1 == (*l2 as i128) =>
        {
            Some((Value::Int(*h1), Value::Int(*l2), false))
        }
        (CoordSpan::Int { lo: l1, .. }, CoordSpan::Int { hi: h2, .. })
            if (*h2 as i128) + 1 == (*l1 as i128) =>
        {
            Some((Value::Int(*h2), Value::Int(*l1), true))
        }
        (CoordSpan::BoolMask(m1), CoordSpan::BoolMask(m2)) => {
            if (*m1 & 0b01) != 0 && (*m2 & 0b10) != 0 {
                Some((Value::Bool(false), Value::Bool(true), false))
            } else if (*m2 & 0b01) != 0 && (*m1 & 0b10) != 0 {
                Some((Value::Bool(false), Value::Bool(true), true))
            } else {
                None
            }
        }
        (CoordSpan::EnumSet(v1), CoordSpan::EnumSet(v2)) => {
            // Find any distinct pair `(a, b)` with `a in v1`, `b in v2`, `a != b`.
            for &a in v1 {
                for &b in v2 {
                    if a != b {
                        if a < b {
                            return Some((Value::Enum(a), Value::Enum(b), false));
                        } else {
                            return Some((Value::Enum(b), Value::Enum(a), true));
                        }
                    }
                }
            }
            None
        }
        _ => None,
    }
}
