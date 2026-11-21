//! JSON output for the CLI's `--json` flag.

use crate::cliffs::CliffEdge;
use crate::diff::DiffReport;
use crate::model::{Domain, Schema, Value};

/// Renders cliff edges as JSON, for example to store as regression test cases.
///
/// Output is deterministic for a given policy. Inputs are objects keyed by field name.
pub fn cliffs_to_json(schema: &Schema, cliffs: &[CliffEdge]) -> String {
    let mut out = String::from("{\n  \"cliff_edges\": [\n");
    for (i, c) in cliffs.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str(&format!(
            "      \"field\": \"{}\",\n",
            escape(&c.field_name)
        ));
        out.push_str(&format!(
            "      \"severity_delta\": {},\n",
            c.severity_delta
        ));
        out.push_str(&format!(
            "      \"unbuffered_seam\": {},\n",
            c.unbuffered_seam
        ));
        out.push_str(&format!(
            "      \"outcome_a\": {{\"label\": \"{}\", \"severity\": {}, \"rule\": {}}},\n",
            escape(&c.outcome_a.label),
            c.outcome_a.severity,
            opt_str(c.rule_a.as_deref())
        ));
        out.push_str(&format!(
            "      \"input_a\": {},\n",
            tuple_to_json(schema, &c.input_a)
        ));
        out.push_str(&format!(
            "      \"outcome_b\": {{\"label\": \"{}\", \"severity\": {}, \"rule\": {}}},\n",
            escape(&c.outcome_b.label),
            c.outcome_b.severity,
            opt_str(c.rule_b.as_deref())
        ));
        out.push_str(&format!(
            "      \"input_b\": {}\n",
            tuple_to_json(schema, &c.input_b)
        ));
        if i + 1 < cliffs.len() {
            out.push_str("    },\n");
        } else {
            out.push_str("    }\n");
        }
    }
    out.push_str("  ]\n}\n");
    out
}

/// Renders a diff report as JSON.
///
/// The volume fields are strings, because counts above 2^53 do not survive a round
/// trip through many JSON parsers as numbers.
pub fn diff_to_json(schema: &Schema, diff: &DiffReport) -> String {
    let mut out = String::from("{\n");
    out.push_str(&format!("  \"is_equivalent\": {},\n", diff.is_equivalent));
    out.push_str(&format!("  \"total_volume\": \"{}\",\n", diff.total_volume));
    out.push_str(&format!(
        "  \"unchanged_volume\": \"{}\",\n",
        diff.unchanged_volume
    ));
    out.push_str(&format!(
        "  \"changed_volume\": \"{}\",\n",
        diff.changed_volume
    ));
    out.push_str("  \"classes\": [\n");
    for (i, cls) in diff.classes.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str(&format!(
            "      \"old\": {{\"label\": \"{}\", \"severity\": {}, \"rule\": {}}},\n",
            escape(&cls.old_outcome.label),
            cls.old_outcome.severity,
            opt_str(cls.old_rule.as_deref())
        ));
        out.push_str(&format!(
            "      \"new\": {{\"label\": \"{}\", \"severity\": {}, \"rule\": {}}},\n",
            escape(&cls.new_outcome.label),
            cls.new_outcome.severity,
            opt_str(cls.new_rule.as_deref())
        ));
        out.push_str(&format!("      \"region_count\": {},\n", cls.region_count));
        out.push_str(&format!(
            "      \"point_volume\": \"{}\",\n",
            cls.point_volume
        ));
        out.push_str(&format!(
            "      \"witness\": {}\n",
            tuple_to_json(schema, &cls.witness)
        ));
        if i + 1 < diff.classes.len() {
            out.push_str("    },\n");
        } else {
            out.push_str("    }\n");
        }
    }
    out.push_str("  ]\n}\n");
    out
}

fn tuple_to_json(schema: &Schema, tuple: &[Value]) -> String {
    let mut parts = Vec::with_capacity(schema.fields.len());
    for (f, v) in schema.fields.iter().zip(tuple.iter()) {
        let rendered = match (&f.domain, v) {
            (Domain::IntRange { .. }, Value::Int(x)) => x.to_string(),
            (Domain::Bool, Value::Bool(b)) => b.to_string(),
            (Domain::Enum { variants }, Value::Enum(idx)) => {
                format!("\"{}\"", escape(&variants[*idx]))
            }
            _ => "\"invalid\"".to_owned(),
        };
        parts.push(format!("\"{}\": {}", escape(&f.name), rendered));
    }
    format!("{{{}}}", parts.join(", "))
}

fn opt_str(s: Option<&str>) -> String {
    match s {
        Some(v) => format!("\"{}\"", escape(v)),
        None => "null".to_owned(),
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // JSON forbids raw control characters inside strings (RFC 8259, section 7).
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
