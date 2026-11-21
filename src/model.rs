use std::fmt;

/// Typed scalar value for a single field of a rule-pipeline input.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Value {
    Int(i64),
    Bool(bool),
    Enum(usize),
}

/// Finite domain for one input coordinate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Domain {
    /// Inclusive integer interval `[min, max]` with `min <= max`.
    IntRange { min: i64, max: i64 },
    /// Boolean coordinate (`false` or `true`).
    Bool,
    /// Finite symbolic enumeration (`variants.len() >= 1`).
    Enum { variants: Vec<String> },
}

impl Domain {
    /// Exact cardinality of this coordinate domain as `u128`.
    pub fn cardinality(&self) -> Option<u128> {
        match self {
            Self::IntRange { min, max } => {
                if max < min {
                    return Some(0);
                }
                let diff = (*max as i128) - (*min as i128) + 1;
                u128::try_from(diff).ok()
            }
            Self::Bool => Some(2),
            Self::Enum { variants } => Some(variants.len() as u128),
        }
    }

    pub fn format_value(&self, v: &Value) -> String {
        match (self, v) {
            (Self::IntRange { .. }, Value::Int(x)) => x.to_string(),
            (Self::Bool, Value::Bool(b)) => b.to_string(),
            (Self::Enum { variants }, Value::Enum(idx)) => variants
                .get(*idx)
                .cloned()
                .unwrap_or_else(|| format!("enum#{idx}")),
            _ => format!("{v:?}"),
        }
    }
}

/// Named field in a pipeline input schema.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldDef {
    pub name: String,
    pub domain: Domain,
}

/// Ordered input tuple schema `D_0 x D_1 x ... x D_{n-1}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schema {
    pub fields: Vec<FieldDef>,
}

impl Schema {
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name == name)
    }

    /// Exact total input-space point count across all coordinates, saturating at `u128::MAX` if huge.
    pub fn total_volume(&self) -> u128 {
        let mut prod = 1u128;
        for f in &self.fields {
            let c = f.domain.cardinality().unwrap_or(u128::MAX);
            prod = prod.saturating_mul(c);
        }
        prod
    }

    pub fn validate_input(&self, input: &[Value]) -> bool {
        if input.len() != self.fields.len() {
            return false;
        }
        for (f, v) in self.fields.iter().zip(input.iter()) {
            let ok = match (&f.domain, v) {
                (Domain::IntRange { min, max }, Value::Int(x)) => x >= min && x <= max,
                (Domain::Bool, Value::Bool(_)) => true,
                (Domain::Enum { variants }, Value::Enum(idx)) => *idx < variants.len(),
                _ => false,
            };
            if !ok {
                return false;
            }
        }
        true
    }

    pub fn format_tuple(&self, input: &[Value]) -> String {
        let mut parts = Vec::with_capacity(self.fields.len());
        for (f, v) in self.fields.iter().zip(input.iter()) {
            parts.push(format!("{}={}", f.name, f.domain.format_value(v)));
        }
        format!("{{{}}}", parts.join(", "))
    }
}

/// Relational operator for integer and unit-scaled predicates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl CmpOp {
    #[inline]
    pub fn eval_i128(self, lhs: i128, rhs: i128) -> bool {
        match self {
            Self::Eq => lhs == rhs,
            Self::Ne => lhs != rhs,
            Self::Lt => lhs < rhs,
            Self::Le => lhs <= rhs,
            Self::Gt => lhs > rhs,
            Self::Ge => lhs >= rhs,
        }
    }
}

impl fmt::Display for CmpOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
        };
        f.write_str(s)
    }
}

/// Atomic guard predicate in a rule conjunction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pred {
    /// Compares `IntRange` field against `rhs`.
    IntCmp { field: usize, op: CmpOp, rhs: i64 },
    /// Inclusive range membership `lo <= x[field] <= hi`.
    IntInRange { field: usize, lo: i64, hi: i64 },
    /// Unit-normalized comparison `x[value_field] * scale(x[unit_field]) <op> rhs_scaled`,
    /// where `scales` maps every enum index of `unit_field` to a positive multiplier.
    ScaledIntCmp {
        value_field: usize,
        unit_field: usize,
        scales: Vec<i128>,
        op: CmpOp,
        rhs_scaled: i128,
    },
    /// Compares `Bool` field against `expected`.
    BoolEq { field: usize, expected: bool },
    /// Membership (or non-membership when `negated`) in a subset of `Enum` indices.
    EnumIn {
        field: usize,
        variants: Vec<usize>,
        negated: bool,
    },
}

impl Pred {
    pub fn eval(&self, input: &[Value]) -> bool {
        match self {
            Self::IntCmp { field, op, rhs } => match input.get(*field) {
                Some(Value::Int(x)) => op.eval_i128(*x as i128, *rhs as i128),
                _ => false,
            },
            Self::IntInRange { field, lo, hi } => match input.get(*field) {
                Some(Value::Int(x)) => *x >= *lo && *x <= *hi,
                _ => false,
            },
            Self::ScaledIntCmp {
                value_field,
                unit_field,
                scales,
                op,
                rhs_scaled,
            } => match (input.get(*value_field), input.get(*unit_field)) {
                (Some(Value::Int(v)), Some(Value::Enum(u))) => {
                    let scale = scales.get(*u).copied().unwrap_or(0);
                    op.eval_i128((*v as i128) * scale, *rhs_scaled)
                }
                _ => false,
            },
            Self::BoolEq { field, expected } => match input.get(*field) {
                Some(Value::Bool(b)) => *b == *expected,
                _ => false,
            },
            Self::EnumIn {
                field,
                variants,
                negated,
            } => match input.get(*field) {
                Some(Value::Enum(idx)) => {
                    let mem = variants.contains(idx);
                    if *negated {
                        !mem
                    } else {
                        mem
                    }
                }
                _ => false,
            },
        }
    }
}

/// Decision outcome carrying a symbolic label and ordinal severity (`0..=255`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Outcome {
    pub label: String,
    pub severity: u8,
}

impl Outcome {
    pub fn new(label: impl Into<String>, severity: u8) -> Self {
        Self {
            label: label.into(),
            severity,
        }
    }
}

/// Single conjunctive guard rule evaluated in priority order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub id: String,
    pub when_all: Vec<Pred>,
    pub outcome: Outcome,
}

/// Deterministic first-match rule pipeline over a typed input `Schema`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pipeline {
    pub name: String,
    pub schema: Schema,
    pub rules: Vec<Rule>,
    pub default_outcome: Outcome,
}

impl Pipeline {
    /// Evaluates `input` directly against the ordered rules, returning `(outcome, matched_rule_id)`.
    pub fn eval<'a>(&'a self, input: &[Value]) -> (&'a Outcome, Option<&'a str>) {
        debug_assert!(self.schema.validate_input(input));
        for rule in &self.rules {
            if rule.when_all.iter().all(|p| p.eval(input)) {
                return (&rule.outcome, Some(rule.id.as_str()));
            }
        }
        (&self.default_outcome, None)
    }
}
