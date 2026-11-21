//! Data types for parsed policies: fields, guards, rules and outcomes.

use std::fmt;

/// The value of one input field.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Value {
    /// Value of an `int` field.
    Int(i64),
    /// Value of a `bool` field.
    Bool(bool),
    /// Index into the field's enum variant list (in declaration order).
    Enum(usize),
}

/// The set of values one field can take.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Domain {
    /// Inclusive integer interval `[min, max]` with `min <= max`.
    IntRange { min: i64, max: i64 },
    /// Boolean coordinate (`false` or `true`).
    Bool,
    /// Named variants, at least one, in declaration order.
    Enum { variants: Vec<String> },
}

impl Domain {
    /// Number of values in this domain. Never `None` for domains built by the parser.
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

    /// Renders `v` the way it would be written in a policy file.
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

/// A named input field, from a `field` line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldDef {
    pub name: String,
    pub domain: Domain,
}

/// The input fields of a pipeline, in declaration order. An input is one value per field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schema {
    pub fields: Vec<FieldDef>,
}

impl Schema {
    /// Position of the field called `name`.
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name == name)
    }

    /// Number of possible inputs (the product of the field domain sizes).
    ///
    /// Saturates at `u128::MAX` (about 3.4e38) for very large schemas, such as three
    /// fields that each span the whole `i64` range.
    pub fn total_volume(&self) -> u128 {
        let mut prod = 1u128;
        for f in &self.fields {
            let c = f.domain.cardinality().unwrap_or(u128::MAX);
            prod = prod.saturating_mul(c);
        }
        prod
    }

    /// True when `input` has one value per field and each value is in its field's domain.
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

    /// Renders `input` as `{name=value, ...}`.
    pub fn format_tuple(&self, input: &[Value]) -> String {
        let mut parts = Vec::with_capacity(self.fields.len());
        for (f, v) in self.fields.iter().zip(input.iter()) {
            parts.push(format!("{}={}", f.name, f.domain.format_value(v)));
        }
        format!("{{{}}}", parts.join(", "))
    }
}

/// Comparison operator for integer and `scaled(...)` guards.
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
    /// Applies the operator to `lhs` and `rhs`.
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

/// One guard line inside a rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pred {
    /// `field <op> rhs` on an `int` field.
    IntCmp { field: usize, op: CmpOp, rhs: i64 },
    /// `field in lo..hi`, inclusive at both ends: `lo <= x[field] <= hi`.
    IntInRange { field: usize, lo: i64, hi: i64 },
    /// `scaled(...)`: compares `x[value_field] * scale(x[unit_field]) <op> rhs_scaled`,
    /// where `scales` maps every enum index of `unit_field` to a positive multiplier.
    ///
    /// Every product `x[value_field] * scale` over the value field's domain must fit in `i128`.
    /// `parse_policy` rejects multipliers that break this; callers building `Pred` values
    /// directly must uphold it themselves.
    ScaledIntCmp {
        value_field: usize,
        unit_field: usize,
        scales: Vec<i128>,
        op: CmpOp,
        rhs_scaled: i128,
    },
    /// `field == expected` on a `bool` field (`!=` is stored as the negated value).
    BoolEq { field: usize, expected: bool },
    /// `field in a,b` on an enum field, or `not_in` when `negated`. `==` and `!=` use a
    /// one-element list.
    EnumIn {
        field: usize,
        variants: Vec<usize>,
        negated: bool,
    },
}

impl Pred {
    /// Evaluates the guard on one input. Returns `false` if a field has the wrong type.
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

/// What a rule (or the default) returns: a label and a severity from 0 to 255.
///
/// Two outcomes are equal only if both the label and the severity match.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Outcome {
    pub label: String,
    pub severity: u8,
}

impl Outcome {
    /// Creates an outcome.
    pub fn new(label: impl Into<String>, severity: u8) -> Self {
        Self {
            label: label.into(),
            severity,
        }
    }
}

/// A rule: it matches when every guard in `when_all` holds. A rule with no guards always matches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub id: String,
    pub when_all: Vec<Pred>,
    pub outcome: Outcome,
}

/// A parsed policy: rules are tried in order and the first match wins; if none match,
/// `default_outcome` applies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pipeline {
    pub name: String,
    pub schema: Schema,
    pub rules: Vec<Rule>,
    pub default_outcome: Outcome,
}

impl Pipeline {
    /// Evaluates one input, returning the outcome and the id of the matching rule
    /// (`None` for the default). This is the reference that the other functions are
    /// tested against.
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
