const SCOPE_SEPARATOR: char = '\u{1}';

fn windows_path(key: &str) -> bool {
    let path = key.split(SCOPE_SEPARATOR).next().unwrap_or(key);
    cfg!(windows) || path.as_bytes().get(1) == Some(&b':') || path.starts_with("\\\\")
}

pub fn normalize_usage_source_key(key: &str) -> String {
    if !windows_path(key) {
        return key.to_owned();
    }
    let (path, scope) = key
        .split_once(SCOPE_SEPARATOR)
        .map_or((key, None), |(path, scope)| (path, Some(scope)));
    let path = path.replace('\\', "/");
    match scope {
        Some(scope) => format!("{path}{SCOPE_SEPARATOR}{scope}"),
        None => path,
    }
}

pub(super) fn source_key_sql(column: &str, key: &str) -> String {
    if !windows_path(key) {
        return column.to_owned();
    }
    format!("replace(substr({column}, 1, instr({column} || char(1), char(1)) - 1), char(92), '/') || substr({column}, instr({column} || char(1), char(1)))")
}

#[cfg(test)]
mod tests;
