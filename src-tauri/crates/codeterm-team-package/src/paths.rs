use crate::diagnostics::DiagnosticCode;

pub(crate) const ROOT_DOCUMENT: &str = "team.yaml";

const WINDOWS_RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Checks a package-relative path: forward slashes, no escape, portable on every OS.
pub(crate) fn check(path: &str, max_bytes: usize) -> Result<(), (DiagnosticCode, String)> {
    if path.is_empty() {
        return Err((DiagnosticCode::InvalidPath, "a path cannot be empty".into()));
    }
    if path.starts_with('/') || path.starts_with('\\') || path.as_bytes().get(1) == Some(&b':') {
        return Err((
            DiagnosticCode::AbsolutePath,
            format!("`{path}` must be relative to the package root"),
        ));
    }
    if path.contains('\\') {
        return Err((
            DiagnosticCode::InvalidPath,
            format!("`{path}` must use forward slashes"),
        ));
    }
    if path.len() > max_bytes {
        return Err((
            DiagnosticCode::InvalidPath,
            format!("`{path}` exceeds {max_bytes} bytes"),
        ));
    }
    for component in path.split('/') {
        if component == ".." {
            return Err((
                DiagnosticCode::PathEscape,
                format!("`{path}` escapes the package root"),
            ));
        }
        if component.is_empty() || component == "." {
            return Err((
                DiagnosticCode::InvalidPath,
                format!("`{path}` has an empty or `.` component"),
            ));
        }
        if component
            .chars()
            .any(|character| character.is_control() || ":*?\"<>|".contains(character))
            || component.ends_with([' ', '.'])
        {
            return Err((
                DiagnosticCode::InvalidPath,
                format!("`{path}` contains a character some systems cannot store"),
            ));
        }
        let stem = component.split('.').next().unwrap_or(component);
        if WINDOWS_RESERVED.contains(&stem.to_ascii_lowercase().as_str()) {
            return Err((
                DiagnosticCode::InvalidPath,
                format!("`{path}` uses a reserved device name"),
            ));
        }
    }
    Ok(())
}

/// True when `path` is `dir` itself or inside it.
pub(crate) fn within(path: &str, dir: &str) -> bool {
    path == dir
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
}
