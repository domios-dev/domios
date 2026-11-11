//! `atp-validate`: the standalone ATP reference validator and schema exporter.

use std::io::Write;
use std::path::Path;

use codeterm_team_package::{
    check_path, schema_document, CheckOptions, DiagnosticCode, HOST_PROFILES,
};

const USAGE: &str = "usage: atp-validate <path> [--host-profile none|domios] [--json]
       atp-validate schema --spec-version <YYYY-MM-DD>
";

const EXIT_VALID: i32 = 0;
const EXIT_INVALID: i32 = 1;
const EXIT_USAGE_OR_IO: i32 = 2;

enum Command {
    Help,
    Check {
        path: String,
        options: CheckOptions,
        json: bool,
    },
    Schema {
        spec_version: String,
    },
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = run(
        &args,
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    std::process::exit(code);
}

/// Runs one invocation and returns its exit code: 0 valid, 1 errors, 2 usage or IO error.
fn run(args: &[String], out: &mut impl Write, err: &mut impl Write) -> i32 {
    let command = match parse(args) {
        Ok(command) => command,
        Err(message) => {
            let _ = write!(err, "atp-validate: {message}\n{USAGE}");
            return EXIT_USAGE_OR_IO;
        }
    };
    let written = match command {
        Command::Help => out.write_all(USAGE.as_bytes()).map(|()| EXIT_VALID),
        Command::Schema { spec_version } => write_schema(&spec_version, out),
        Command::Check {
            path,
            options,
            json,
        } => write_check(&path, &options, json, out),
    };
    written.unwrap_or_else(|error| {
        let _ = writeln!(err, "atp-validate: {error}");
        EXIT_USAGE_OR_IO
    })
}

fn parse(args: &[String]) -> Result<Command, String> {
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        return Ok(Command::Help);
    }
    if args.first().map(String::as_str) == Some("schema") {
        return parse_schema(&args[1..]);
    }
    let mut path = None;
    let mut profile = "none".to_string();
    let mut json = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--json" => json = true,
            "--host-profile" => profile = value_of(arg, rest.next())?,
            flag if flag.starts_with('-') && flag != "-" => {
                return Err(format!("unknown option {flag}"))
            }
            _ if path.is_some() => return Err(format!("unexpected argument {arg}")),
            _ => path = Some(arg.clone()),
        }
    }
    let path = path.ok_or("missing package path")?;
    let options = CheckOptions::for_profile(&profile).ok_or_else(|| {
        format!(
            "unknown host profile {profile}; expected one of {}",
            HOST_PROFILES.join(", ")
        )
    })?;
    Ok(Command::Check {
        path,
        options,
        json,
    })
}

fn parse_schema(args: &[String]) -> Result<Command, String> {
    let mut spec_version = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--spec-version" => spec_version = Some(value_of(arg, rest.next())?),
            _ => return Err(format!("unexpected argument {arg}")),
        }
    }
    let spec_version = spec_version.ok_or("missing --spec-version")?;
    if !is_spec_version(&spec_version) {
        return Err(format!(
            "spec version {spec_version} is not a YYYY-MM-DD date"
        ));
    }
    Ok(Command::Schema { spec_version })
}

fn value_of(flag: &str, value: Option<&String>) -> Result<String, String> {
    value
        .filter(|value| !value.starts_with("--"))
        .cloned()
        .ok_or_else(|| format!("{flag} needs a value"))
}

fn is_spec_version(text: &str) -> bool {
    text.len() == 10
        && text.char_indices().all(|(index, character)| match index {
            4 | 7 => character == '-',
            _ => character.is_ascii_digit(),
        })
}

fn write_schema(spec_version: &str, out: &mut impl Write) -> std::io::Result<i32> {
    let text = serde_json::to_string_pretty(&schema_document(spec_version))?;
    writeln!(out, "{text}")?;
    Ok(EXIT_VALID)
}

fn write_check(
    path: &str,
    options: &CheckOptions,
    json: bool,
    out: &mut impl Write,
) -> std::io::Result<i32> {
    let report = check_path(Path::new(path), options).report;
    if json {
        writeln!(out, "{}", report.to_json())?;
    } else {
        out.write_all(report.render(path).as_bytes())?;
    }
    let unreadable = report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == DiagnosticCode::Unreadable);
    Ok(if unreadable {
        EXIT_USAGE_OR_IO
    } else if report.valid {
        EXIT_VALID
    } else {
        EXIT_INVALID
    })
}

#[cfg(test)]
mod tests;
