//! Parser for the `.policy` text format.

use std::fmt;

use crate::model::{CmpOp, Domain, FieldDef, Outcome, Pipeline, Pred, Rule, Schema};

/// A policy file that could not be parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based line number. Errors about the whole file (such as a missing header) use line 1.
    pub line: usize,
    /// What went wrong.
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

/// Parses the text of a `.policy` file into a [`Pipeline`].
///
/// The syntax is described in the crate README. Anything the parser cannot read
/// unambiguously is an error, with the 1-based line number in [`ParseError::line`].
pub fn parse_policy(src: &str) -> Result<Pipeline, ParseError> {
    let mut name: Option<String> = None;
    let mut default_outcome: Option<Outcome> = None;
    let mut fields: Vec<FieldDef> = Vec::new();
    let mut rules: Vec<Rule> = Vec::new();
    let mut current_rule: Option<Rule> = None;

    for (idx, raw_line) in src.lines().enumerate() {
        let ln = idx + 1;
        let line = strip_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix("pipeline ") {
            let pname = rest.trim();
            if pname.is_empty() {
                return Err(err(ln, "missing pipeline name"));
            }
            if name.is_some() {
                return Err(err(ln, "duplicate 'pipeline' header"));
            }
            name = Some(pname.to_owned());
            continue;
        }

        if let Some(rest) = line.strip_prefix("default ") {
            if default_outcome.is_some() {
                return Err(err(ln, "duplicate 'default' outcome"));
            }
            default_outcome = Some(parse_outcome_spec(ln, rest.trim())?);
            continue;
        }

        if let Some(rest) = line.strip_prefix("field ") {
            if current_rule.is_some() {
                return Err(err(ln, "all field declarations must precede rules"));
            }
            let field = parse_field(ln, rest.trim())?;
            if fields.iter().any(|f| f.name == field.name) {
                return Err(err(ln, format!("duplicate field '{}'", field.name)));
            }
            fields.push(field);
            continue;
        }

        if let Some(rest) = line.strip_prefix("rule ") {
            if let Some(done) = current_rule.take() {
                rules.push(done);
            }
            let header = rest
                .trim()
                .strip_suffix(':')
                .ok_or_else(|| err(ln, "rule header must end with ':'"))?;
            let (rid, out_spec) = header
                .split_once("->")
                .ok_or_else(|| err(ln, "expected 'rule <id> -> <Label> severity=<n>:'"))?;
            let id = rid.trim();
            if id.is_empty() {
                return Err(err(ln, "empty rule id"));
            }
            if rules.iter().any(|r| r.id == id) {
                return Err(err(ln, format!("duplicate rule id '{id}'")));
            }
            let outcome = parse_outcome_spec(ln, out_spec.trim())?;
            current_rule = Some(Rule {
                id: id.to_owned(),
                when_all: Vec::new(),
                outcome,
            });
            continue;
        }

        let Some(ref mut rule) = current_rule else {
            return Err(err(
                ln,
                format!("unexpected statement outside rule: {line}"),
            ));
        };
        let schema_ref = Schema {
            fields: fields.clone(),
        };
        let pred = parse_pred(ln, &schema_ref, line)?;
        rule.when_all.push(pred);
    }

    if let Some(done) = current_rule.take() {
        rules.push(done);
    }

    let pname = name.ok_or_else(|| err(1, "missing 'pipeline <name>' header"))?;
    let def_out =
        default_outcome.ok_or_else(|| err(1, "missing 'default <Label> severity=<n>'"))?;
    if fields.is_empty() {
        return Err(err(1, "schema must declare at least one field"));
    }

    Ok(Pipeline {
        name: pname,
        schema: Schema { fields },
        rules,
        default_outcome: def_out,
    })
}

fn strip_comment(s: &str) -> &str {
    match s.find('#') {
        Some(i) => &s[..i],
        None => s,
    }
}

fn err(line: usize, msg: impl Into<String>) -> ParseError {
    ParseError {
        line,
        message: msg.into(),
    }
}

fn parse_outcome_spec(ln: usize, spec: &str) -> Result<Outcome, ParseError> {
    let mut parts = spec.split_whitespace();
    let label = parts
        .next()
        .ok_or_else(|| err(ln, "missing outcome label"))?;
    let mut severity: Option<u8> = None;
    for tok in parts {
        if let Some(sev_str) = tok.strip_prefix("severity=") {
            if severity.is_some() {
                return Err(err(ln, "duplicate 'severity=' attribute"));
            }
            severity = Some(
                sev_str
                    .parse::<u8>()
                    .map_err(|_| err(ln, format!("invalid severity '{sev_str}'")))?,
            );
        } else {
            return Err(err(ln, format!("unexpected outcome attribute '{tok}'")));
        }
    }
    Ok(Outcome::new(label, severity.unwrap_or(0)))
}

const KEYWORDS: [&str; 4] = ["pipeline", "default", "field", "rule"];
const RESERVED_CHARS: &str = "=<>!,()";

fn parse_field(ln: usize, spec: &str) -> Result<FieldDef, ParseError> {
    let mut parts = spec.split_whitespace();
    let name = parts
        .next()
        .ok_or_else(|| err(ln, "missing field name"))?
        .to_owned();
    // A guard line is parsed by its leading keyword and its operator, so a field named
    // `rule` or `a<b` would turn its own guards into something else.
    if KEYWORDS.contains(&name.as_str()) {
        return Err(err(ln, format!("'{name}' is a keyword, not a field name")));
    }
    if let Some(bad) = name.chars().find(|c| RESERVED_CHARS.contains(*c)) {
        return Err(err(
            ln,
            format!("field name '{name}' must not contain '{bad}'"),
        ));
    }
    let kind = parts
        .next()
        .ok_or_else(|| err(ln, "missing field type ('int', 'bool', or 'enum')"))?;
    let domain = match kind {
        "bool" => Domain::Bool,
        "int" => {
            let range_tok = parts
                .next()
                .ok_or_else(|| err(ln, "expected '<min>..<max>' for int field"))?;
            let (min_s, max_s) = range_tok
                .split_once("..")
                .ok_or_else(|| err(ln, "expected '<min>..<max>'"))?;
            let min = min_s
                .parse::<i64>()
                .map_err(|_| err(ln, format!("invalid integer min '{min_s}'")))?;
            let max = max_s
                .parse::<i64>()
                .map_err(|_| err(ln, format!("invalid integer max '{max_s}'")))?;
            if max < min {
                return Err(err(ln, "int field max must be >= min"));
            }
            Domain::IntRange { min, max }
        }
        "enum" => {
            // The variant list is the rest of the line, so `a, b` and `a,b` both declare
            // two variants. A space without a comma (`a b`) is rejected below.
            let list: Vec<&str> = parts.by_ref().collect();
            if list.is_empty() {
                return Err(err(ln, "expected comma-separated enum variants"));
            }
            let joined = list.join(" ");
            let mut variants: Vec<String> = Vec::new();
            for item in joined.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                if item.contains(char::is_whitespace) {
                    return Err(err(
                        ln,
                        format!("invalid enum variant '{item}': separate variants with commas"),
                    ));
                }
                if variants.iter().any(|v| v == item) {
                    return Err(err(ln, format!("duplicate enum variant '{item}'")));
                }
                variants.push(item.to_owned());
            }
            if variants.is_empty() {
                return Err(err(ln, "enum must list at least one variant"));
            }
            Domain::Enum { variants }
        }
        other => return Err(err(ln, format!("unknown field type '{other}'"))),
    };
    if let Some(extra) = parts.next() {
        return Err(err(
            ln,
            format!("unexpected token '{extra}' in field declaration"),
        ));
    }
    Ok(FieldDef { name, domain })
}

fn parse_pred(ln: usize, schema: &Schema, line: &str) -> Result<Pred, ParseError> {
    if let Some(rest) = line.strip_prefix("scaled(") {
        return parse_scaled_pred(ln, schema, rest);
    }

    // Check `in` and `not_in` before relational operators.
    if let Some((lhs, rhs)) = line.split_once(" not_in ") {
        let f_idx = lookup_field(ln, schema, lhs.trim())?;
        return parse_membership(ln, schema, f_idx, rhs.trim(), true);
    }
    if let Some((lhs, rhs)) = line.split_once(" in ") {
        let f_idx = lookup_field(ln, schema, lhs.trim())?;
        return parse_membership(ln, schema, f_idx, rhs.trim(), false);
    }

    for (tok, op) in [
        (">=", CmpOp::Ge),
        ("<=", CmpOp::Le),
        ("==", CmpOp::Eq),
        ("!=", CmpOp::Ne),
        (">", CmpOp::Gt),
        ("<", CmpOp::Lt),
    ] {
        if let Some((lhs, rhs)) = line.split_once(tok) {
            let f_idx = lookup_field(ln, schema, lhs.trim())?;
            let rhs_trim = rhs.trim();
            return match &schema.fields[f_idx].domain {
                Domain::IntRange { .. } => {
                    let val = rhs_trim
                        .parse::<i64>()
                        .map_err(|_| err(ln, format!("invalid integer literal '{rhs_trim}'")))?;
                    Ok(Pred::IntCmp {
                        field: f_idx,
                        op,
                        rhs: val,
                    })
                }
                Domain::Bool => {
                    let b = rhs_trim.parse::<bool>().map_err(|_| {
                        err(ln, format!("expected 'true' or 'false', got '{rhs_trim}'"))
                    })?;
                    match op {
                        CmpOp::Eq => Ok(Pred::BoolEq {
                            field: f_idx,
                            expected: b,
                        }),
                        CmpOp::Ne => Ok(Pred::BoolEq {
                            field: f_idx,
                            expected: !b,
                        }),
                        _ => Err(err(ln, "boolean fields only support == and !=")),
                    }
                }
                Domain::Enum { variants } => {
                    let v_idx = variants
                        .iter()
                        .position(|v| v == rhs_trim)
                        .ok_or_else(|| err(ln, format!("unknown enum variant '{rhs_trim}'")))?;
                    match op {
                        CmpOp::Eq => Ok(Pred::EnumIn {
                            field: f_idx,
                            variants: vec![v_idx],
                            negated: false,
                        }),
                        CmpOp::Ne => Ok(Pred::EnumIn {
                            field: f_idx,
                            variants: vec![v_idx],
                            negated: true,
                        }),
                        _ => Err(err(ln, "enum fields only support ==, !=, in, and not_in")),
                    }
                }
            };
        }
    }

    Err(err(ln, format!("unrecognized predicate syntax: {line}")))
}

fn parse_membership(
    ln: usize,
    schema: &Schema,
    f_idx: usize,
    rhs: &str,
    negated: bool,
) -> Result<Pred, ParseError> {
    match &schema.fields[f_idx].domain {
        Domain::IntRange { .. } => {
            if negated {
                return Err(err(ln, "int range predicates do not support not_in"));
            }
            let (lo_s, hi_s) = rhs
                .split_once("..")
                .ok_or_else(|| err(ln, "expected '<lo>..<hi>' after 'in'"))?;
            let lo = lo_s
                .parse::<i64>()
                .map_err(|_| err(ln, format!("invalid range lo '{lo_s}'")))?;
            let hi = hi_s
                .parse::<i64>()
                .map_err(|_| err(ln, format!("invalid range hi '{hi_s}'")))?;
            if hi < lo {
                return Err(err(ln, "range hi must be >= lo"));
            }
            Ok(Pred::IntInRange {
                field: f_idx,
                lo,
                hi,
            })
        }
        Domain::Enum { variants } => {
            let mut indices = Vec::new();
            for item in rhs.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                let idx = variants
                    .iter()
                    .position(|v| v == item)
                    .ok_or_else(|| err(ln, format!("unknown enum variant '{item}'")))?;
                if !indices.contains(&idx) {
                    indices.push(idx);
                }
            }
            if indices.is_empty() {
                return Err(err(
                    ln,
                    "expected at least one enum variant after 'in'/'not_in'",
                ));
            }
            indices.sort_unstable();
            Ok(Pred::EnumIn {
                field: f_idx,
                variants: indices,
                negated,
            })
        }
        Domain::Bool => Err(err(ln, "bool fields do not support 'in'")),
    }
}

fn parse_scaled_pred(ln: usize, schema: &Schema, rest: &str) -> Result<Pred, ParseError> {
    let (inside, tail) = rest
        .split_once(')')
        .ok_or_else(|| err(ln, "unclosed 'scaled(...)'"))?;
    let mut args = inside.split(',').map(str::trim);
    let vf_name = args
        .next()
        .ok_or_else(|| err(ln, "missing value field in scaled(...)"))?;
    let uf_name = args
        .next()
        .ok_or_else(|| err(ln, "missing unit field in scaled(...)"))?;
    let value_field = lookup_field(ln, schema, vf_name)?;
    let unit_field = lookup_field(ln, schema, uf_name)?;

    let Domain::IntRange {
        min: value_min,
        max: value_max,
    } = schema.fields[value_field].domain
    else {
        return Err(err(ln, "scaled(...) value field must have type int"));
    };
    let Domain::Enum { ref variants } = schema.fields[unit_field].domain else {
        return Err(err(ln, "scaled(...) unit field must have type enum"));
    };

    let mut scales = vec![0i128; variants.len()];
    for mapping in args {
        let (vname, mult_s) = mapping.split_once('=').ok_or_else(|| {
            err(
                ln,
                format!("expected '<unit>=<multiplier>', got '{mapping}'"),
            )
        })?;
        let v_idx = variants
            .iter()
            .position(|v| v == vname.trim())
            .ok_or_else(|| err(ln, format!("unknown unit variant '{}'", vname.trim())))?;
        let mult = mult_s
            .trim()
            .parse::<i128>()
            .map_err(|_| err(ln, format!("invalid scale multiplier '{mult_s}'")))?;
        if mult <= 0 {
            return Err(err(ln, "scale multipliers must be positive"));
        }
        // Every product `value * multiplier` over the value field's domain must fit in `i128`;
        // the endpoints bound all intermediate values because the product is monotone.
        if (value_min as i128).checked_mul(mult).is_none()
            || (value_max as i128).checked_mul(mult).is_none()
        {
            return Err(err(
                ln,
                format!(
                    "scale multiplier '{}' overflows 128-bit arithmetic over field '{vf_name}'",
                    mult_s.trim()
                ),
            ));
        }
        if scales[v_idx] != 0 {
            return Err(err(
                ln,
                format!("duplicate multiplier for unit '{}'", vname.trim()),
            ));
        }
        scales[v_idx] = mult;
    }
    if scales.contains(&0) {
        return Err(err(
            ln,
            "scaled(...) must provide a multiplier for every unit variant",
        ));
    }

    let tail_trim = tail.trim();
    for (tok, op) in [
        (">=", CmpOp::Ge),
        ("<=", CmpOp::Le),
        ("==", CmpOp::Eq),
        ("!=", CmpOp::Ne),
        (">", CmpOp::Gt),
        ("<", CmpOp::Lt),
    ] {
        if let Some(rhs_s) = tail_trim.strip_prefix(tok) {
            let rhs_scaled = rhs_s
                .trim()
                .parse::<i128>()
                .map_err(|_| err(ln, format!("invalid scaled threshold '{}'", rhs_s.trim())))?;
            return Ok(Pred::ScaledIntCmp {
                value_field,
                unit_field,
                scales,
                op,
                rhs_scaled,
            });
        }
    }

    Err(err(ln, "missing comparison operator after scaled(...)"))
}

fn lookup_field(ln: usize, schema: &Schema, name: &str) -> Result<usize, ParseError> {
    schema
        .field_index(name)
        .ok_or_else(|| err(ln, format!("unknown field '{name}'")))
}
