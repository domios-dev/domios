use super::{path_exists, write_file_atomically};
use rusqlite::{Connection, OpenFlags, Transaction};
use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy)]
enum Encoding {
    Plain,
    Json,
}

struct StoredColumn {
    table: &'static str,
    column: &'static str,
    encoding: Encoding,
    primary_key: bool,
}

const fn plain(table: &'static str, column: &'static str) -> StoredColumn {
    StoredColumn {
        table,
        column,
        encoding: Encoding::Plain,
        primary_key: false,
    }
}

const fn json(table: &'static str, column: &'static str) -> StoredColumn {
    StoredColumn {
        table,
        column,
        encoding: Encoding::Json,
        primary_key: false,
    }
}

const fn primary_key(table: &'static str, column: &'static str) -> StoredColumn {
    StoredColumn {
        table,
        column,
        encoding: Encoding::Plain,
        primary_key: true,
    }
}

const DATABASES: &[(&str, &[StoredColumn])] = &[
    (
        "storage/state.db",
        &[
            plain("workspaces", "cwd"),
            plain("workspaces", "provisioned_cwd"),
            plain("desk_layout_nodes", "cwd"),
            json("desk_layout_nodes", "fields_json"),
            plain("cron_jobs", "cwd"),
            plain("cron_runs", "cwd"),
            plain("cron_last_run_mirrors", "cwd"),
            plain("blueprints", "cwd"),
        ],
    ),
    (
        "plans.db",
        &[
            plain("plans", "project_path"),
            json("plans", "document_json"),
            plain("specs", "project_path"),
            json("specs", "document_json"),
            primary_key("legacy_imports", "root"),
        ],
    ),
    (
        "closed_history.db",
        &[
            plain("closed_group", "workspace_cwd"),
            plain("closed_tab", "cwd"),
            json("closed_tab", "backing_json"),
            json("closed_tab", "launch_json"),
            json("closed_tab", "runtime_config_json"),
            json("closed_structure", "payload"),
        ],
    ),
    (
        "mem/knowledge.db",
        &[
            plain("mem_items", "source"),
            plain("mem_items", "source_path"),
            plain("mem_items", "body_blob_path"),
        ],
    ),
];

const JSON_FILES: &[&str] = &[
    "crons.json",
    "config.json",
    "plugin-channels.json",
    "mem/workers.json",
];

/// Rewrites a path spelled under the legacy data root to the same place under its renamed sibling.
pub(super) struct PathRebase {
    legacy: PathBuf,
    pattern: Vec<char>,
    name_offset: usize,
    replacement: String,
    windows: bool,
    home_root: bool,
}

impl PathRebase {
    pub(super) fn new(legacy: &Path, current: &Path) -> Option<Self> {
        Self::with_platform(legacy, current, cfg!(windows))
    }

    fn with_platform(legacy: &Path, current: &Path, windows: bool) -> Option<Self> {
        let current = normalized_spelling(current, windows)?;
        let replacement = current.rsplit('/').next()?.to_string();
        let pattern: Vec<char> = normalized_spelling(legacy, windows)?.chars().collect();
        let name_offset = pattern.iter().rposition(|&character| character == '/')? + 1;
        (name_offset < pattern.len() && !replacement.is_empty()).then(|| Self {
            legacy: legacy.to_path_buf(),
            pattern,
            name_offset,
            replacement,
            windows,
            home_root: dirs::home_dir().is_some_and(|home| {
                legacy
                    .parent()
                    .and_then(|parent| normalized_spelling(parent, windows))
                    == normalized_spelling(&home, windows)
            }),
        })
    }

    pub(super) fn rebase_path(&self, path: &Path) -> PathBuf {
        path.to_str()
            .and_then(|value| self.rebase_value(value))
            .map_or_else(|| path.to_path_buf(), PathBuf::from)
    }

    fn needle(&self) -> String {
        self.pattern[self.name_offset..]
            .iter()
            .collect::<String>()
            .to_lowercase()
    }

    fn rebase(&self, encoding: Encoding, text: &str) -> Option<String> {
        match encoding {
            Encoding::Plain => self.rebase_value(text),
            Encoding::Json => self.rebase_json(text),
        }
    }

    fn rebase_value(&self, value: &str) -> Option<String> {
        let start = self.skip_prefixes(value, 0, false);
        let span = self.match_root(value, start, false)?;
        Some(self.splice(value, &[span]))
    }

    fn rebase_json(&self, text: &str) -> Option<String> {
        let bytes = text.as_bytes();
        let mut spans = Vec::new();
        let mut in_string = false;
        let mut index = 0;
        while index < bytes.len() {
            match (in_string, bytes[index]) {
                (false, b'"') => {
                    in_string = true;
                    let start = self.skip_prefixes(text, index + 1, true);
                    spans.extend(self.match_root(text, start, true));
                }
                (true, b'\\') => index += 1,
                (true, b'"') => in_string = false,
                _ => {}
            }
            index += 1;
        }
        (!spans.is_empty()).then(|| self.splice(text, &spans))
    }

    fn splice(&self, text: &str, spans: &[(usize, usize)]) -> String {
        let mut rebased = String::with_capacity(text.len());
        let mut copied = 0;
        for &(name_start, name_end) in spans {
            rebased.push_str(&text[copied..name_start]);
            rebased.push_str(&self.replacement);
            copied = name_end;
        }
        rebased.push_str(&text[copied..]);
        rebased
    }

    fn skip_prefixes(&self, text: &str, start: usize, json: bool) -> usize {
        let mut at = start;
        if text
            .get(at..at + 5)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("file:"))
        {
            at += 5;
        }
        if self.windows {
            let verbatim: &[&str] = if json {
                &[r"\\\\?\\", "//?/"]
            } else {
                &[r"\\?\", "//?/"]
            };
            if let Some(prefix) = verbatim
                .iter()
                .find(|prefix| text[at..].starts_with(**prefix))
            {
                at += prefix.len();
            }
        }
        at
    }

    fn match_root(&self, text: &str, start: usize, json: bool) -> Option<(usize, usize)> {
        let mut at = start;
        let mut name_start = at;
        let home_pattern: Vec<char>;
        let (pattern, name_offset) = if text[start..].starts_with("~/") {
            if !self.home_root {
                return None;
            }
            home_pattern = [vec!['~', '/'], self.pattern[self.name_offset..].to_vec()].concat();
            (home_pattern.as_slice(), 2)
        } else {
            (self.pattern.as_slice(), self.name_offset)
        };
        for (index, &expected) in pattern.iter().enumerate() {
            if index == name_offset {
                name_start = at;
            }
            if expected == '/' {
                at += self.separator_len(&text[at..], json)?;
            } else {
                let actual = text[at..].chars().next()?;
                if !self.same_char(actual, expected) {
                    return None;
                }
                at += actual.len_utf8();
            }
        }
        let rest = &text[at..];
        let terminated = if json {
            rest.starts_with('"')
        } else {
            rest.is_empty()
        };
        (terminated || self.separator_len(rest, json).is_some()).then_some((name_start, at))
    }

    fn separator_len(&self, text: &str, json: bool) -> Option<usize> {
        if text.starts_with('/') {
            Some(1)
        } else if json && (text.starts_with(r"\/") || (self.windows && text.starts_with(r"\\"))) {
            Some(2)
        } else if self.windows && !json && text.starts_with('\\') {
            Some(1)
        } else {
            None
        }
    }

    fn same_char(&self, actual: char, expected: char) -> bool {
        actual == expected || (self.windows && actual.to_lowercase().eq(expected.to_lowercase()))
    }
}

fn normalized_spelling(path: &Path, windows: bool) -> Option<String> {
    let mut spelled = path.to_str()?.to_string();
    if windows {
        spelled = if let Some(unc) = spelled.strip_prefix(r"\\?\UNC\") {
            format!(r"\\{unc}")
        } else if let Some(local) = spelled.strip_prefix(r"\\?\") {
            local.to_string()
        } else {
            spelled
        }
        .replace('\\', "/");
    }
    Some(spelled.trim_end_matches('/').to_string())
}

#[derive(Debug, Default)]
struct RebaseOutcome {
    rewritten: usize,
    notices: Vec<String>,
}

pub(super) fn rebase_stored_paths(data: &Path, rebase: &PathRebase) -> Vec<String> {
    let outcome = rebase_stores(data, rebase);
    if outcome.rewritten > 0 {
        log::info!(
            "[upgrade] rebased {} stored paths from {} to {}",
            outcome.rewritten,
            rebase.legacy.display(),
            data.display()
        );
    }
    outcome.notices
}

fn rebase_stores(data: &Path, rebase: &PathRebase) -> RebaseOutcome {
    let mut outcome = RebaseOutcome::default();
    match path_exists(&rebase.legacy) {
        Ok(false) => {}
        Ok(true) => return outcome,
        Err(error) => {
            outcome.notices.push(format!(
                "Stored paths still point into {}: cannot check whether it exists: {error}",
                rebase.legacy.display()
            ));
            return outcome;
        }
    }
    for (relative, columns) in DATABASES {
        let path = data.join(relative);
        if !path.is_file() {
            continue;
        }
        match rebase_database(&path, columns, rebase) {
            Ok(rewritten) => outcome.rewritten += rewritten,
            Err(error) => outcome
                .notices
                .push(stale_paths_notice(&path, rebase, error)),
        }
    }
    for relative in JSON_FILES {
        let path = data.join(relative);
        match rebase_json_file(&path, rebase) {
            Ok(rewritten) => outcome.rewritten += usize::from(rewritten),
            Err(error) => outcome
                .notices
                .push(stale_paths_notice(&path, rebase, error)),
        }
    }
    outcome
}

fn stale_paths_notice(path: &Path, rebase: &PathRebase, error: impl std::fmt::Display) -> String {
    format!(
        "Stored paths in {} still point into {}: {error}",
        path.display(),
        rebase.legacy.display()
    )
}

fn rebase_database(
    path: &Path,
    columns: &[StoredColumn],
    rebase: &PathRebase,
) -> rusqlite::Result<usize> {
    let mut connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    let transaction = connection.transaction()?;
    let needle = rebase.needle();
    let mut rewritten = 0;
    for column in columns {
        if !has_column(&transaction, column)? {
            continue;
        }
        let rows = {
            let mut statement = transaction.prepare(&format!(
                "SELECT rowid, \"{name}\" FROM \"{table}\" \
                 WHERE typeof(\"{name}\") = 'text' AND instr(lower(\"{name}\"), ?1) > 0",
                name = column.column,
                table = column.table,
            ))?;
            let rows = statement.query_map([&needle], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (rowid, value) in rows {
            if let Some(rebased) = rebase.rebase(column.encoding, &value) {
                write_column(&transaction, column, rowid, &rebased)?;
                rewritten += 1;
            }
        }
    }
    transaction.commit()?;
    Ok(rewritten)
}

fn has_column(transaction: &Transaction<'_>, column: &StoredColumn) -> rusqlite::Result<bool> {
    transaction.query_row(
        "SELECT EXISTS (SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)",
        [column.table, column.column],
        |row| row.get(0),
    )
}

fn write_column(
    transaction: &Transaction<'_>,
    column: &StoredColumn,
    rowid: i64,
    value: &str,
) -> rusqlite::Result<()> {
    let conflict = if column.primary_key { " OR IGNORE" } else { "" };
    let updated = transaction.execute(
        &format!(
            "UPDATE{conflict} \"{table}\" SET \"{name}\" = ?1 WHERE rowid = ?2",
            table = column.table,
            name = column.column,
        ),
        rusqlite::params![value, rowid],
    )?;
    if column.primary_key && updated == 0 {
        // The rebased key is already recorded, so the legacy spelling is a duplicate.
        transaction.execute(
            &format!("DELETE FROM \"{}\" WHERE rowid = ?1", column.table),
            [rowid],
        )?;
    }
    Ok(())
}

pub(super) fn rebase_json_file(path: &Path, rebase: &PathRebase) -> io::Result<bool> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    serde_json::from_str::<serde_json::Value>(&text)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let Some(rebased) = rebase.rebase_json(&text) else {
        return Ok(false);
    };
    write_file_atomically(path, rebased.as_bytes())?;
    Ok(true)
}

#[cfg(test)]
mod tests;
