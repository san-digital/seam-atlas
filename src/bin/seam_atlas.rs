use std::env;
use std::fs;
use std::io::{self, Write};
use std::process::ExitCode;

use seam_atlas::{
    cliffs_to_json, compile_partition, diff_pipelines, diff_to_json, find_cliffs_in_partition,
    parse_policy, Domain, Pipeline, Schema,
};

/// Exit code when `diff --fail-on-diff` finds inputs whose outcome changed.
const EXIT_CHANGED: u8 = 1;
/// Exit code for usage errors, unreadable files, parse errors and schema mismatches.
const EXIT_ERROR: u8 = 2;

const USAGE: &str = "\
usage:
  seam-atlas diff [--json] [--fail-on-diff] <old-policy> <new-policy>
  seam-atlas cliffs [--json] <policy-file>
  seam-atlas --help | --version
";

const HELP: &str = "\
seam-atlas: check every input of a first-match rules file, not a sample.

commands:
  diff <old> <new>    List every input whose outcome differs between two
                      versions of a policy, grouped by (old rule, new rule),
                      with one example input per group.
  cliffs <policy>     List places where changing one field by one step (an
                      integer by 1, a bool, or an enum variant) changes the
                      outcome. Jumps of 2 or more severity levels are marked
                      UNBUFFERED.

options:
  --json              Print machine-readable JSON instead of text.
  --fail-on-diff      (diff only) Exit with status 1 if any outcome changed.
  -h, --help          Print this help.
  -V, --version       Print the version.

exit status:
  0  success (for diff without --fail-on-diff, also when outcomes changed)
  1  diff --fail-on-diff found changed outcomes
  2  bad arguments, unreadable file, parse error, or the two policies
     declare different fields

Policy syntax: https://github.com/san-digital/seam-atlas#policy-syntax
";

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(first) = args.first() else {
        eprint!("{USAGE}");
        return ExitCode::from(EXIT_ERROR);
    };

    match first.as_str() {
        "cliffs" => run_cliffs(&args[1..]),
        "diff" => run_diff(&args[1..]),
        "-h" | "--help" | "help" => emit(HELP.to_owned()),
        "-V" | "--version" => emit(format!("seam-atlas {}\n", env!("CARGO_PKG_VERSION"))),
        other => usage_error(&format!("unknown command '{other}'")),
    }
}

fn usage_error(msg: &str) -> ExitCode {
    eprint!("error: {msg}\n{USAGE}");
    ExitCode::from(EXIT_ERROR)
}

fn fail(msg: &str) -> ExitCode {
    eprintln!("error: {msg}");
    ExitCode::from(EXIT_ERROR)
}

/// Writes `out` to stdout. A closed pipe (for example `| head`) is not an error.
fn write_out(out: &str) -> Result<(), ExitCode> {
    let mut stdout = io::stdout().lock();
    match stdout
        .write_all(out.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(fail(&format!("cannot write output: {e}"))),
    }
}

fn emit(out: String) -> ExitCode {
    match write_out(&out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => code,
    }
}

fn load(path: &str) -> Result<Pipeline, ExitCode> {
    let src = fs::read_to_string(path).map_err(|e| fail(&format!("cannot read '{path}': {e}")))?;
    parse_policy(&src).map_err(|e| fail(&format!("{path}:{e}")))
}

fn run_cliffs(args: &[String]) -> ExitCode {
    let mut json_mode = false;
    let mut path: Option<&str> = None;
    for arg in args {
        match arg.as_str() {
            "--json" => json_mode = true,
            "-h" | "--help" => return emit(HELP.to_owned()),
            other if other.starts_with('-') => {
                return usage_error(&format!("unknown option '{other}'"));
            }
            other if path.is_none() => path = Some(other),
            other => return usage_error(&format!("unexpected argument '{other}'")),
        }
    }
    let Some(p) = path else {
        return usage_error("missing policy file");
    };
    let pipeline = match load(p) {
        Ok(pl) => pl,
        Err(code) => return code,
    };

    let part = compile_partition(&pipeline);
    let cliffs = find_cliffs_in_partition(&pipeline, &part);
    if json_mode {
        return emit(cliffs_to_json(&pipeline.schema, &cliffs));
    }

    let unbuffered = cliffs.iter().filter(|c| c.unbuffered_seam).count();
    let mut out = format!(
        "pipeline '{}': {} inputs fall into {} regions with a fixed outcome\n\
         found {} cliff edges ({} unbuffered, severity jumps by 2 or more):\n",
        pipeline.name,
        part.total_volume(),
        part.regions.len(),
        cliffs.len(),
        unbuffered
    );
    for c in &cliffs {
        let tag = if c.unbuffered_seam {
            "[UNBUFFERED]"
        } else {
            "[adjacent]  "
        };
        out += &format!(
            "  {} field={} delta_sev={}: {} ({}) -> {} ({})\n",
            tag,
            c.field_name,
            c.severity_delta,
            c.outcome_a.label,
            pipeline.schema.format_tuple(&c.input_a),
            c.outcome_b.label,
            pipeline.schema.format_tuple(&c.input_b)
        );
    }
    emit(out)
}

fn run_diff(args: &[String]) -> ExitCode {
    let mut json_mode = false;
    let mut fail_on_diff = false;
    let mut paths: Vec<&str> = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--json" => json_mode = true,
            "--fail-on-diff" => fail_on_diff = true,
            "-h" | "--help" => return emit(HELP.to_owned()),
            other if other.starts_with('-') => {
                return usage_error(&format!("unknown option '{other}'"));
            }
            other => paths.push(other),
        }
    }
    if paths.len() != 2 {
        return usage_error("expected <old-policy> and <new-policy>");
    }

    let old_p = match load(paths[0]) {
        Ok(p) => p,
        Err(code) => return code,
    };
    let new_p = match load(paths[1]) {
        Ok(p) => p,
        Err(code) => return code,
    };

    let Some(report) = diff_pipelines(&old_p, &new_p) else {
        return fail(&format!(
            "'{}' and '{}' declare different fields: {}",
            paths[0],
            paths[1],
            schema_mismatch(&old_p.schema, &new_p.schema)
        ));
    };

    let out = if json_mode {
        diff_to_json(&old_p.schema, &report)
    } else if report.is_equivalent {
        format!(
            "EQUIVALENT: all {} inputs get the same outcome.\n",
            report.total_volume
        )
    } else {
        let mut out = format!(
            "CHANGED: {} of {} inputs get a different outcome, in {} group(s):\n",
            report.changed_volume,
            report.total_volume,
            report.classes.len()
        );
        for cls in &report.classes {
            out += &format!(
                "  {} (sev={}, rule {}) -> {} (sev={}, rule {}) [inputs={}, regions={}] example={}\n",
                cls.old_outcome.label,
                cls.old_outcome.severity,
                cls.old_rule.as_deref().unwrap_or("default"),
                cls.new_outcome.label,
                cls.new_outcome.severity,
                cls.new_rule.as_deref().unwrap_or("default"),
                cls.point_volume,
                cls.region_count,
                old_p.schema.format_tuple(&cls.witness)
            );
        }
        out
    };

    if let Err(code) = write_out(&out) {
        return code;
    }
    if fail_on_diff && !report.is_equivalent {
        ExitCode::from(EXIT_CHANGED)
    } else {
        ExitCode::SUCCESS
    }
}

/// Describes the first difference between two schemas, for the mismatch error.
fn schema_mismatch(old: &Schema, new: &Schema) -> String {
    for (i, (a, b)) in old.fields.iter().zip(&new.fields).enumerate() {
        if a != b {
            return format!(
                "field {} is '{} {}' in the old policy but '{} {}' in the new one \
                 (names, types, ranges and enum variant order must match)",
                i + 1,
                a.name,
                describe(&a.domain),
                b.name,
                describe(&b.domain)
            );
        }
    }
    format!(
        "the old policy has {} fields, the new one has {}",
        old.fields.len(),
        new.fields.len()
    )
}

fn describe(domain: &Domain) -> String {
    match domain {
        Domain::IntRange { min, max } => format!("int {min}..{max}"),
        Domain::Bool => "bool".to_owned(),
        Domain::Enum { variants } => format!("enum {}", variants.join(",")),
    }
}
