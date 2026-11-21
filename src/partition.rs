//! Splitting the input space into regions with a fixed outcome.

use crate::model::{CmpOp, Domain, Outcome, Pipeline, Pred, Rule, Schema, Value};

/// The values one field takes inside a [`LeafRegion`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoordSpan {
    /// Inclusive integer interval `[lo, hi]`.
    Int { lo: i64, hi: i64 },
    /// Subset of boolean values (`0b01` = false, `0b10` = true, `0b11` = both).
    BoolMask(u8),
    /// Non-empty sorted list of enum variant indices.
    EnumSet(Vec<usize>),
}

impl CoordSpan {
    /// The span covering the whole domain.
    pub fn full_from_domain(domain: &Domain) -> Self {
        match domain {
            Domain::IntRange { min, max } => Self::Int { lo: *min, hi: *max },
            Domain::Bool => Self::BoolMask(0b11),
            Domain::Enum { variants } => Self::EnumSet((0..variants.len()).collect()),
        }
    }

    /// Number of values in the span.
    pub fn cardinality(&self) -> u128 {
        match self {
            Self::Int { lo, hi } => {
                if hi < lo {
                    0
                } else {
                    ((*hi as i128) - (*lo as i128)) as u128 + 1
                }
            }
            Self::BoolMask(m) => (m & 0b11).count_ones() as u128,
            Self::EnumSet(vs) => vs.len() as u128,
        }
    }

    /// A fixed example value from this (non-empty) span: the smallest integer, `false`
    /// before `true`, or the first enum variant.
    pub fn witness(&self) -> Value {
        match self {
            Self::Int { lo, .. } => Value::Int(*lo),
            Self::BoolMask(m) => {
                if (m & 0b01) != 0 {
                    Value::Bool(false)
                } else {
                    Value::Bool(true)
                }
            }
            Self::EnumSet(vs) => Value::Enum(vs[0]),
        }
    }

    /// An example value that lies in both `self` and `other`, if they overlap.
    pub fn shared_witness(&self, other: &Self) -> Option<Value> {
        match (self, other) {
            (Self::Int { lo: l1, hi: h1 }, Self::Int { lo: l2, hi: h2 }) => {
                let lo = (*l1).max(*l2);
                let hi = (*h1).min(*h2);
                if lo <= hi {
                    Some(Value::Int(lo))
                } else {
                    None
                }
            }
            (Self::BoolMask(m1), Self::BoolMask(m2)) => {
                let both = m1 & m2 & 0b11;
                if (both & 0b01) != 0 {
                    Some(Value::Bool(false))
                } else if (both & 0b10) != 0 {
                    Some(Value::Bool(true))
                } else {
                    None
                }
            }
            (Self::EnumSet(v1), Self::EnumSet(v2)) => {
                for &x in v1 {
                    if v2.binary_search(&x).is_ok() {
                        return Some(Value::Enum(x));
                    }
                }
                None
            }
            _ => None,
        }
    }
}

/// A box of inputs (one [`CoordSpan`] per field) that all get the same outcome from the
/// same rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeafRegion {
    pub spans: Vec<CoordSpan>,
    pub outcome: Outcome,
    pub matched_rule: Option<String>,
}

impl LeafRegion {
    /// Number of inputs in the region (saturating).
    pub fn volume(&self) -> u128 {
        let mut prod = 1u128;
        for s in &self.spans {
            prod = prod.saturating_mul(s.cardinality());
        }
        prod
    }

    /// An example input from the region, built from each span's [`CoordSpan::witness`].
    pub fn canonical_witness(&self) -> Vec<Value> {
        self.spans.iter().map(CoordSpan::witness).collect()
    }
}

/// The regions of a pipeline: every input lies in exactly one region.
#[derive(Clone, Debug)]
pub struct Partition {
    pub schema: Schema,
    pub regions: Vec<LeafRegion>,
}

impl Partition {
    /// Sum of the region volumes. Equals [`Schema::total_volume`] unless that saturates.
    pub fn total_volume(&self) -> u128 {
        let mut sum = 0u128;
        for r in &self.regions {
            sum = sum.saturating_add(r.volume());
        }
        sum
    }
}

/// Three-valued evaluation of an atomic predicate over a box of `CoordSpan`s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TriEval {
    AlwaysTrue,
    AlwaysFalse,
    Split { field: usize },
}

pub(crate) fn eval_pred_on_box(pred: &Pred, spans: &[CoordSpan]) -> TriEval {
    match pred {
        Pred::IntCmp { field, op, rhs } => {
            let CoordSpan::Int { lo, hi } = spans[*field] else {
                return TriEval::AlwaysFalse;
            };
            eval_i128_interval(lo as i128, hi as i128, *op, *rhs as i128, *field)
        }
        Pred::IntInRange { field, lo, hi } => {
            let CoordSpan::Int { lo: blo, hi: bhi } = spans[*field] else {
                return TriEval::AlwaysFalse;
            };
            if blo >= *lo && bhi <= *hi {
                TriEval::AlwaysTrue
            } else if bhi < *lo || blo > *hi {
                TriEval::AlwaysFalse
            } else {
                TriEval::Split { field: *field }
            }
        }
        Pred::ScaledIntCmp {
            value_field,
            unit_field,
            scales,
            op,
            rhs_scaled,
        } => {
            let CoordSpan::EnumSet(ref units) = spans[*unit_field] else {
                return TriEval::AlwaysFalse;
            };
            if units.len() > 1 {
                // Refine unit_field into singletons before testing scaled thresholds.
                return TriEval::Split { field: *unit_field };
            }
            let u = units[0];
            let scale = scales.get(u).copied().unwrap_or(0);
            let CoordSpan::Int { lo, hi } = spans[*value_field] else {
                return TriEval::AlwaysFalse;
            };
            if scale != 0
                && matches!(op, CmpOp::Eq | CmpOp::Ne)
                && rhs_scaled.rem_euclid(scale.abs()) != 0
            {
                return if *op == CmpOp::Eq {
                    TriEval::AlwaysFalse
                } else {
                    TriEval::AlwaysTrue
                };
            }
            let prod_a = (lo as i128) * scale;
            let prod_b = (hi as i128) * scale;
            let (min_p, max_p) = if prod_a <= prod_b {
                (prod_a, prod_b)
            } else {
                (prod_b, prod_a)
            };
            eval_i128_interval(min_p, max_p, *op, *rhs_scaled, *value_field)
        }
        Pred::BoolEq { field, expected } => {
            let CoordSpan::BoolMask(m) = spans[*field] else {
                return TriEval::AlwaysFalse;
            };
            let want = if *expected { 0b10 } else { 0b01 };
            let other = want ^ 0b11;
            if (m & other) == 0 && (m & want) != 0 {
                TriEval::AlwaysTrue
            } else if (m & want) == 0 {
                TriEval::AlwaysFalse
            } else {
                TriEval::Split { field: *field }
            }
        }
        Pred::EnumIn {
            field,
            variants,
            negated,
        } => {
            let CoordSpan::EnumSet(ref cur) = spans[*field] else {
                return TriEval::AlwaysFalse;
            };
            let mut any_in = false;
            let mut any_out = false;
            for &idx in cur {
                if variants.contains(&idx) {
                    any_in = true;
                } else {
                    any_out = true;
                }
            }
            let (any_true, any_false) = if *negated {
                (any_out, any_in)
            } else {
                (any_in, any_out)
            };
            if any_true && !any_false {
                TriEval::AlwaysTrue
            } else if !any_true {
                TriEval::AlwaysFalse
            } else {
                TriEval::Split { field: *field }
            }
        }
    }
}

fn eval_i128_interval(min_v: i128, max_v: i128, op: CmpOp, rhs: i128, field: usize) -> TriEval {
    match op {
        CmpOp::Eq => {
            if min_v == rhs && max_v == rhs {
                TriEval::AlwaysTrue
            } else if max_v < rhs || min_v > rhs {
                TriEval::AlwaysFalse
            } else {
                TriEval::Split { field }
            }
        }
        CmpOp::Ne => {
            if max_v < rhs || min_v > rhs {
                TriEval::AlwaysTrue
            } else if min_v == rhs && max_v == rhs {
                TriEval::AlwaysFalse
            } else {
                TriEval::Split { field }
            }
        }
        CmpOp::Lt => {
            if max_v < rhs {
                TriEval::AlwaysTrue
            } else if min_v >= rhs {
                TriEval::AlwaysFalse
            } else {
                TriEval::Split { field }
            }
        }
        CmpOp::Le => {
            if max_v <= rhs {
                TriEval::AlwaysTrue
            } else if min_v > rhs {
                TriEval::AlwaysFalse
            } else {
                TriEval::Split { field }
            }
        }
        CmpOp::Gt => {
            if min_v > rhs {
                TriEval::AlwaysTrue
            } else if max_v <= rhs {
                TriEval::AlwaysFalse
            } else {
                TriEval::Split { field }
            }
        }
        CmpOp::Ge => {
            if min_v >= rhs {
                TriEval::AlwaysTrue
            } else if max_v < rhs {
                TriEval::AlwaysFalse
            } else {
                TriEval::Split { field }
            }
        }
    }
}

/// Splits a single coordinate span using the first unresolved predicate `pred`.
pub(crate) fn split_box_for_pred(spans: &[CoordSpan], pred: &Pred) -> Vec<Vec<CoordSpan>> {
    match pred {
        Pred::IntCmp { field, op, rhs } => split_int_cmp(spans, *field, *op, *rhs),
        Pred::IntInRange { field, lo, hi } => {
            let CoordSpan::Int { lo: blo, hi: bhi } = spans[*field] else {
                return vec![];
            };
            let mut pieces = Vec::with_capacity(3);
            if blo < *lo {
                pieces.push((blo, (*lo - 1).min(bhi)));
            }
            let mid_lo = blo.max(*lo);
            let mid_hi = bhi.min(*hi);
            if mid_lo <= mid_hi {
                pieces.push((mid_lo, mid_hi));
            }
            if bhi > *hi {
                pieces.push(((*hi + 1).max(blo), bhi));
            }
            pieces
                .into_iter()
                .map(|(a, b)| replace_span(spans, *field, CoordSpan::Int { lo: a, hi: b }))
                .collect()
        }
        Pred::ScaledIntCmp {
            value_field,
            unit_field,
            scales,
            op,
            rhs_scaled,
        } => {
            if let CoordSpan::EnumSet(ref units) = spans[*unit_field] {
                if units.len() > 1 {
                    return units
                        .iter()
                        .map(|&u| replace_span(spans, *unit_field, CoordSpan::EnumSet(vec![u])))
                        .collect();
                }
                let u = units[0];
                let scale = scales.get(u).copied().unwrap_or(0);
                if scale > 0 {
                    // Convert `v * scale <op> rhs_scaled` into an exact integer split on `v`.
                    return split_scaled_positive(spans, *value_field, scale, *op, *rhs_scaled);
                } else if scale < 0 {
                    let flipped_op = match *op {
                        CmpOp::Eq => CmpOp::Eq,
                        CmpOp::Ne => CmpOp::Ne,
                        CmpOp::Lt => CmpOp::Gt,
                        CmpOp::Le => CmpOp::Ge,
                        CmpOp::Gt => CmpOp::Lt,
                        CmpOp::Ge => CmpOp::Le,
                    };
                    return split_scaled_positive(
                        spans,
                        *value_field,
                        -scale,
                        flipped_op,
                        -*rhs_scaled,
                    );
                }
            }
            vec![]
        }
        Pred::BoolEq { field, .. } => vec![
            replace_span(spans, *field, CoordSpan::BoolMask(0b01)),
            replace_span(spans, *field, CoordSpan::BoolMask(0b10)),
        ],
        Pred::EnumIn {
            field, variants, ..
        } => {
            let CoordSpan::EnumSet(ref cur) = spans[*field] else {
                return vec![];
            };
            let mut inside = Vec::new();
            let mut outside = Vec::new();
            for &v in cur {
                if variants.contains(&v) {
                    inside.push(v);
                } else {
                    outside.push(v);
                }
            }
            let mut out = Vec::with_capacity(2);
            if !inside.is_empty() {
                out.push(replace_span(spans, *field, CoordSpan::EnumSet(inside)));
            }
            if !outside.is_empty() {
                out.push(replace_span(spans, *field, CoordSpan::EnumSet(outside)));
            }
            out
        }
    }
}

fn split_int_cmp(spans: &[CoordSpan], field: usize, op: CmpOp, rhs: i64) -> Vec<Vec<CoordSpan>> {
    let CoordSpan::Int { lo, hi } = spans[field] else {
        return vec![];
    };
    let mut intervals = Vec::with_capacity(3);
    match op {
        CmpOp::Eq | CmpOp::Ne => {
            if lo < rhs {
                intervals.push((lo, rhs - 1));
            }
            if lo <= rhs && rhs <= hi {
                intervals.push((rhs, rhs));
            }
            if rhs < hi {
                intervals.push((rhs + 1, hi));
            }
        }
        CmpOp::Lt | CmpOp::Ge => {
            // Split into `[lo, rhs - 1]` and `[rhs, hi]`.
            if lo < rhs {
                intervals.push((lo, (rhs - 1).min(hi)));
            }
            if rhs <= hi {
                intervals.push((rhs.max(lo), hi));
            }
        }
        CmpOp::Le | CmpOp::Gt => {
            // Split into `[lo, rhs]` and `[rhs + 1, hi]`.
            if lo <= rhs {
                intervals.push((lo, rhs.min(hi)));
            }
            if rhs < hi {
                intervals.push(((rhs + 1).max(lo), hi));
            }
        }
    }
    intervals
        .into_iter()
        .map(|(a, b)| replace_span(spans, field, CoordSpan::Int { lo: a, hi: b }))
        .collect()
}

fn split_scaled_positive(
    spans: &[CoordSpan],
    field: usize,
    scale: i128,
    op: CmpOp,
    rhs_scaled: i128,
) -> Vec<Vec<CoordSpan>> {
    let CoordSpan::Int { lo, hi } = spans[field] else {
        return vec![];
    };
    // Exact floor and ceiling divisions of rhs_scaled by positive `scale`.
    let q_floor = rhs_scaled.div_euclid(scale);
    let rem = rhs_scaled.rem_euclid(scale);
    let q_ceil = if rem == 0 { q_floor } else { q_floor + 1 };

    match op {
        CmpOp::Eq | CmpOp::Ne => {
            if rem != 0 {
                return vec![spans.to_vec()];
            }
            let Ok(target) = i64::try_from(q_floor) else {
                return vec![spans.to_vec()];
            };
            split_int_cmp(spans, field, op, target)
        }
        CmpOp::Ge | CmpOp::Lt => {
            // `v * scale >= rhs_scaled` iff `v >= ceil(rhs_scaled / scale)`.
            if lo >= hi {
                return vec![spans.to_vec()];
            }
            let thresh = q_ceil.clamp((lo as i128) + 1, hi as i128) as i64;
            split_int_cmp(spans, field, CmpOp::Ge, thresh)
        }
        CmpOp::Gt | CmpOp::Le => {
            // `v * scale <= rhs_scaled` iff `v <= floor(rhs_scaled / scale)`.
            if lo >= hi {
                return vec![spans.to_vec()];
            }
            let thresh = q_floor.clamp(lo as i128, (hi as i128) - 1) as i64;
            split_int_cmp(spans, field, CmpOp::Le, thresh)
        }
    }
}

fn replace_span(spans: &[CoordSpan], field: usize, next: CoordSpan) -> Vec<CoordSpan> {
    let mut copy = spans.to_vec();
    copy[field] = next;
    copy
}

/// Splits the whole input space of `pipeline` into regions that each get a single outcome.
///
/// The regions do not overlap and together contain every input. The work depends on how
/// many regions the rules create, not on how many inputs there are. That number is small
/// for typical policies but can grow exponentially with many overlapping rules.
pub fn compile_partition(pipeline: &Pipeline) -> Partition {
    let root: Vec<CoordSpan> = pipeline
        .schema
        .fields
        .iter()
        .map(|f| CoordSpan::full_from_domain(&f.domain))
        .collect();
    let active_indices: Vec<usize> = (0..pipeline.rules.len()).collect();
    let mut regions = Vec::new();
    refine_single(pipeline, root, &active_indices, &mut regions);
    Partition {
        schema: pipeline.schema.clone(),
        regions,
    }
}

fn refine_single(
    pipeline: &Pipeline,
    spans: Vec<CoordSpan>,
    candidate_rules: &[usize],
    out: &mut Vec<LeafRegion>,
) {
    let mut surviving = Vec::with_capacity(candidate_rules.len());
    let mut split_pred: Option<&Pred> = None;

    for &r_idx in candidate_rules {
        let rule: &Rule = &pipeline.rules[r_idx];
        let mut rule_false = false;
        let mut first_unresolved: Option<&Pred> = None;

        for p in &rule.when_all {
            match eval_pred_on_box(p, &spans) {
                TriEval::AlwaysTrue => {}
                TriEval::AlwaysFalse => {
                    rule_false = true;
                    break;
                }
                TriEval::Split { .. } => {
                    if first_unresolved.is_none() {
                        first_unresolved = Some(p);
                    }
                }
            }
        }

        if rule_false {
            continue;
        }

        if surviving.is_empty() && first_unresolved.is_none() {
            // Highest-priority surviving rule holds across the entire box.
            out.push(LeafRegion {
                spans,
                outcome: rule.outcome.clone(),
                matched_rule: Some(rule.id.clone()),
            });
            return;
        }

        if split_pred.is_none() {
            split_pred = first_unresolved;
        }
        surviving.push(r_idx);
    }

    let Some(pred) = split_pred else {
        // No rule survived on this box: fallback default outcome applies everywhere on `spans`.
        out.push(LeafRegion {
            spans,
            outcome: pipeline.default_outcome.clone(),
            matched_rule: None,
        });
        return;
    };

    for child_spans in split_box_for_pred(&spans, pred) {
        refine_single(pipeline, child_spans, &surviving, out);
    }
}
