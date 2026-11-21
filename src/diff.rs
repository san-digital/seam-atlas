use std::collections::BTreeMap;

use crate::model::{Outcome, Pipeline, Pred, Value};
use crate::partition::{eval_pred_on_box, split_box_for_pred, CoordSpan, TriEval};

/// Canonical equivalence class of inputs whose behavior changed from `old` to `new`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffClass {
    pub old_outcome: Outcome,
    pub old_rule: Option<String>,
    pub new_outcome: Outcome,
    pub new_rule: Option<String>,
    pub region_count: usize,
    pub point_volume: u128,
    pub witness: Vec<Value>,
}

/// Complete behavioral diff report across 100% of the shared input `Schema`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffReport {
    pub is_equivalent: bool,
    pub total_volume: u128,
    pub unchanged_volume: u128,
    pub changed_volume: u128,
    pub classes: Vec<DiffClass>,
}

/// Computes an exhaustive behavioral diff between `old` and `new` over their shared `Schema`.
///
/// Returns `None` if `old.schema != new.schema`.
pub fn diff_pipelines(old: &Pipeline, new: &Pipeline) -> Option<DiffReport> {
    if old.schema != new.schema {
        return None;
    }
    let root: Vec<CoordSpan> = old
        .schema
        .fields
        .iter()
        .map(|f| CoordSpan::full_from_domain(&f.domain))
        .collect();
    let old_idx: Vec<usize> = (0..old.rules.len()).collect();
    let new_idx: Vec<usize> = (0..new.rules.len()).collect();

    let mut acc = DiffAccum::default();
    co_refine(old, new, root, &old_idx, &new_idx, &mut acc);

    let mut classes: Vec<DiffClass> = acc.classes.into_values().collect();
    classes.sort_by(|a, b| {
        b.point_volume
            .cmp(&a.point_volume)
            .then_with(|| a.old_outcome.label.cmp(&b.old_outcome.label))
            .then_with(|| a.new_outcome.label.cmp(&b.new_outcome.label))
    });

    let total = acc.unchanged_volume.saturating_add(acc.changed_volume);
    Some(DiffReport {
        is_equivalent: classes.is_empty(),
        total_volume: total,
        unchanged_volume: acc.unchanged_volume,
        changed_volume: acc.changed_volume,
        classes,
    })
}

type DiffKey = (Outcome, Option<String>, Outcome, Option<String>);

#[derive(Default)]
struct DiffAccum {
    unchanged_volume: u128,
    changed_volume: u128,
    classes: BTreeMap<DiffKey, DiffClass>,
}

enum SideStatus<'a> {
    Resolved(&'a Outcome, Option<&'a str>),
    Pending {
        surviving: Vec<usize>,
        first_pred: &'a Pred,
    },
}

fn step_side<'a>(
    pipeline: &'a Pipeline,
    spans: &[CoordSpan],
    candidates: &[usize],
) -> SideStatus<'a> {
    let mut surviving = Vec::with_capacity(candidates.len());
    let mut split_pred: Option<&'a Pred> = None;

    for &idx in candidates {
        let rule = &pipeline.rules[idx];
        let mut rule_false = false;
        let mut first_unresolved: Option<&'a Pred> = None;

        for p in &rule.when_all {
            match eval_pred_on_box(p, spans) {
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
            return SideStatus::Resolved(&rule.outcome, Some(rule.id.as_str()));
        }
        if split_pred.is_none() {
            split_pred = first_unresolved;
        }
        surviving.push(idx);
    }

    match split_pred {
        Some(pred) => SideStatus::Pending {
            surviving,
            first_pred: pred,
        },
        None => SideStatus::Resolved(&pipeline.default_outcome, None),
    }
}

fn box_volume(spans: &[CoordSpan]) -> u128 {
    let mut prod = 1u128;
    for s in spans {
        prod = prod.saturating_mul(s.cardinality());
    }
    prod
}

fn co_refine(
    old: &Pipeline,
    new: &Pipeline,
    spans: Vec<CoordSpan>,
    old_candidates: &[usize],
    new_candidates: &[usize],
    acc: &mut DiffAccum,
) {
    let st_old = step_side(old, &spans, old_candidates);
    let st_new = step_side(new, &spans, new_candidates);

    match (st_old, st_new) {
        (SideStatus::Resolved(o_out, o_rule), SideStatus::Resolved(n_out, n_rule)) => {
            let vol = box_volume(&spans);
            if o_out == n_out {
                acc.unchanged_volume = acc.unchanged_volume.saturating_add(vol);
            } else {
                acc.changed_volume = acc.changed_volume.saturating_add(vol);
                let witness: Vec<Value> = spans.iter().map(CoordSpan::witness).collect();

                // Verify the concrete witness directly against both pipelines.
                let (chk_o, chk_or) = old.eval(&witness);
                let (chk_n, chk_nr) = new.eval(&witness);
                debug_assert_eq!(chk_o, o_out);
                debug_assert_eq!(chk_n, n_out);
                debug_assert_eq!(chk_or, o_rule);
                debug_assert_eq!(chk_nr, n_rule);

                let key: DiffKey = (
                    o_out.clone(),
                    o_rule.map(str::to_owned),
                    n_out.clone(),
                    n_rule.map(str::to_owned),
                );
                let entry = acc.classes.entry(key).or_insert_with(|| DiffClass {
                    old_outcome: o_out.clone(),
                    old_rule: o_rule.map(str::to_owned),
                    new_outcome: n_out.clone(),
                    new_rule: n_rule.map(str::to_owned),
                    region_count: 0,
                    point_volume: 0,
                    witness,
                });
                entry.region_count += 1;
                entry.point_volume = entry.point_volume.saturating_add(vol);
            }
        }
        (
            SideStatus::Pending {
                surviving: s_old,
                first_pred,
            },
            SideStatus::Pending {
                surviving: s_new, ..
            },
        ) => {
            for child in split_box_for_pred(&spans, first_pred) {
                co_refine(old, new, child, &s_old, &s_new, acc);
            }
        }
        (
            SideStatus::Pending {
                surviving: s_old,
                first_pred,
            },
            SideStatus::Resolved(_, _),
        ) => {
            for child in split_box_for_pred(&spans, first_pred) {
                co_refine(old, new, child, &s_old, new_candidates, acc);
            }
        }
        (
            SideStatus::Resolved(_, _),
            SideStatus::Pending {
                surviving: s_new,
                first_pred,
            },
        ) => {
            for child in split_box_for_pred(&spans, first_pred) {
                co_refine(old, new, child, old_candidates, &s_new, acc);
            }
        }
    }
}
