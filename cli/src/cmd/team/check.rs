use std::path::Path;

use codeterm_team_package::{check_path, CheckOptions, PackageCheckReport, PackageDiagnostic};

use crate::cmd::print::{fail_with, CliError};

fn label(path: &str, diagnostic: &PackageDiagnostic) -> String {
    let package = Path::new(path);
    if package.is_dir()
        || package
            .extension()
            .is_some_and(|extension| extension == "team")
    {
        let file = if diagnostic.file.is_empty() {
            path.to_string()
        } else {
            format!("{path}/{}", diagnostic.file)
        };
        diagnostic.render(&file)
    } else {
        diagnostic.render(path)
    }
}

fn render(path: &str, report: &PackageCheckReport) -> String {
    let mut out = String::new();
    for diagnostic in &report.diagnostics {
        out.push_str(&label(path, diagnostic));
        out.push('\n');
    }
    let identity = match (&report.name, &report.version) {
        (Some(name), Some(version)) => format!(" {name}@{version}"),
        _ => String::new(),
    };
    let errors = report.errors().count();
    let warnings = report.warnings().count();
    if report.valid {
        out.push_str(&format!("valid{identity} ({warnings} warning(s))\n"));
    } else {
        out.push_str(&format!(
            "invalid{identity}: {errors} error(s), {warnings} warning(s)\n"
        ));
    }
    out
}

/// Validates a package in any of its three forms; exits 1 when it has errors.
pub fn handle(json_output: bool, path: &str) {
    let report = check_path(Path::new(path), &CheckOptions::default()).report;
    let value = serde_json::to_value(&report).unwrap_or_default();
    if report.valid {
        if json_output {
            crate::cmd::print::print_json(&value);
        } else {
            print!("{}", render(path, &report));
        }
        return;
    }
    if !json_output {
        print!("{}", render(path, &report));
    }
    fail_with(
        1,
        &CliError {
            message: format!("{path} is not a valid team package"),
            code: None,
            data: Some(value),
        },
    );
}

#[cfg(test)]
mod tests;
