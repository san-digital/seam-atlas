use std::env;
use std::fs;
use std::process::ExitCode;

use seam_atlas::{
    cliffs_to_json, compile_partition, diff_pipelines, diff_to_json, find_cliffs_in_partition,
    parse_policy,
};

fn usage() -> &'static str {
    "usage:\n  seam-atlas cliffs [--json] <policy-file>\n  seam-atlas diff [--json] [--fail-on-diff] <old-policy> <new-policy>\n"
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        eprint!("{}", usage());
        return ExitCode::from(2);
    }

    match args[0].as_str() {
        "cliffs" => run_cliffs(&args[1..]),
        "diff" => run_diff(&args[1..]),
        "-h" | "--help" | "help" => {
            print!("{}", usage());
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("error: unknown subcommand '{other}'\n{}", usage());
            ExitCode::from(2)
        }
    }
}

fn run_cliffs(args: &[String]) -> ExitCode {
    let mut json_mode = false;
    let mut path: Option<&str> = None;
    for arg in args {
        if arg == "--json" {
            json_mode = true;
        } else if arg.starts_with("--") {
            eprint!("error: unknown option '{arg}'\n{}", usage());
            return ExitCode::from(2);
        } else if path.is_none() {
            path = Some(arg.as_str());
        } else {
            eprint!("error: unexpected argument '{arg}'\n{}", usage());
            return ExitCode::from(2);
        }
    }
    let Some(p) = path else {
        eprint!("error: missing policy file\n{}", usage());
        return ExitCode::from(2);
    };
    let src = match fs::read_to_string(p) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read '{p}': {e}");
            return ExitCode::from(1);
        }
    };
    let pipeline = match parse_policy(&src) {
        Ok(pl) => pl,
        Err(e) => {
            eprintln!("error: {p}:{e}");
            return ExitCode::from(1);
        }
    };

    let part = compile_partition(&pipeline);
    let cliffs = find_cliffs_in_partition(&pipeline, &part);
    if json_mode {
        print!("{}", cliffs_to_json(&pipeline.schema, &cliffs));
    } else {
        println!(
            "pipeline '{}' compiled into {} disjoint regions (volume = {})",
            pipeline.name,
            part.regions.len(),
            part.total_volume()
        );
        let unbuffered = cliffs.iter().filter(|c| c.unbuffered_seam).count();
        println!(
            "discovered {} minimal-perturbation cliff edges ({} unbuffered severity jumps >= 2):",
            cliffs.len(),
            unbuffered
        );
        for c in &cliffs {
            let tag = if c.unbuffered_seam {
                "[UNBUFFERED]"
            } else {
                "[adjacent]  "
            };
            println!(
                "  {} field={} delta_sev={}: {} ({}) -> {} ({})",
                tag,
                c.field_name,
                c.severity_delta,
                c.outcome_a.label,
                pipeline.schema.format_tuple(&c.input_a),
                c.outcome_b.label,
                pipeline.schema.format_tuple(&c.input_b)
            );
        }
    }
    ExitCode::SUCCESS
}

fn run_diff(args: &[String]) -> ExitCode {
    let mut json_mode = false;
    let mut fail_on_diff = false;
    let mut paths: Vec<&str> = Vec::new();
    for arg in args {
        match arg.as_str() {
            "--json" => json_mode = true,
            "--fail-on-diff" => fail_on_diff = true,
            other if other.starts_with("--") => {
                eprint!("error: unknown option '{other}'\n{}", usage());
                return ExitCode::from(2);
            }
            _ => paths.push(arg.as_str()),
        }
    }
    if paths.len() != 2 {
        eprint!("error: expected <old-policy> and <new-policy>\n{}", usage());
        return ExitCode::from(2);
    }

    let old_src = match fs::read_to_string(paths[0]) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read '{}': {e}", paths[0]);
            return ExitCode::from(1);
        }
    };
    let new_src = match fs::read_to_string(paths[1]) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: cannot read '{}': {e}", paths[1]);
            return ExitCode::from(1);
        }
    };
    let old_p = match parse_policy(&old_src) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {}:{e}", paths[0]);
            return ExitCode::from(1);
        }
    };
    let new_p = match parse_policy(&new_src) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {}:{e}", paths[1]);
            return ExitCode::from(1);
        }
    };

    let Some(report) = diff_pipelines(&old_p, &new_p) else {
        eprintln!(
            "error: schemas of '{}' and '{}' do not match",
            paths[0], paths[1]
        );
        return ExitCode::from(1);
    };

    if json_mode {
        print!("{}", diff_to_json(&old_p.schema, &report));
    } else if report.is_equivalent {
        println!(
            "EQUIVALENT across all {} input states.",
            report.total_volume
        );
    } else {
        println!(
            "CHANGED: {} of {} states altered across {} equivalence classes:",
            report.changed_volume,
            report.total_volume,
            report.classes.len()
        );
        for cls in &report.classes {
            println!(
                "  {} (sev={}) -> {} (sev={}) [volume={}, regions={}] witness={}",
                cls.old_outcome.label,
                cls.old_outcome.severity,
                cls.new_outcome.label,
                cls.new_outcome.severity,
                cls.point_volume,
                cls.region_count,
                old_p.schema.format_tuple(&cls.witness)
            );
        }
    }

    if fail_on_diff && !report.is_equivalent {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
