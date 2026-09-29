//! `host.sqliteQueryAllJson`: a read-only query streamed into one JSON array
//! whose size, and every value SQLite materialises for it, stay under a cap.

use std::io::Write;

use rusqlite::limits::Limit;
use rusqlite::types::ValueRef;

/// Upper bound for one query's JSON result and for any single SQLite value.
const SQLITE_RESULT_MAX_BYTES: usize = 64 << 20;

pub(super) fn host_query(
    ctx: &rquickjs::Ctx<'_>,
    sandbox: &super::PluginSandbox,
    db: &str,
    sql: &str,
    params_json: Option<&str>,
) -> rquickjs::Result<Option<String>> {
    if !sandbox.can_read(db) {
        log::warn!("[plugin] denied sqliteQuery outside sandbox: {db}");
        return Ok(None);
    }
    query_all_json(db, sql, params_json, SQLITE_RESULT_MAX_BYTES).map_err(|error| {
        log::warn!("[plugin] {error} (db {db})");
        rquickjs::Exception::throw_range(ctx, &error)
    })
}

/// `Ok(None)` keeps the null contract for an unreadable db, bad SQL, or bad
/// params; `Err` is a cap breach the caller surfaces as a JS exception.
fn query_all_json(
    db_path: &str,
    sql: &str,
    params_json: Option<&str>,
    max_bytes: usize,
) -> Result<Option<String>, String> {
    let params: Vec<serde_json::Value> = match params_json {
        Some(j) => match serde_json::from_str(j) {
            Ok(params) => params,
            Err(_) => return Ok(None),
        },
        None => Vec::new(),
    };
    let Ok(conn) = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return Ok(None);
    };
    // SQLite refuses to load a value past this length instead of allocating it.
    conn.set_limit(
        Limit::SQLITE_LIMIT_LENGTH,
        i32::try_from(max_bytes).unwrap_or(i32::MAX),
    );
    let Ok(mut stmt) = conn.prepare(sql) else {
        return Ok(None);
    };
    let col_names: Vec<String> = (0..stmt.column_count())
        .map(|i| {
            stmt.column_name(i)
                .map(|s| s.to_string())
                .unwrap_or_else(|_| format!("col_{i}"))
        })
        .collect();
    let bound: Vec<Box<dyn rusqlite::ToSql>> = params.into_iter().map(bind_param).collect();
    let bound_refs: Vec<&dyn rusqlite::ToSql> = bound.iter().map(|b| b.as_ref()).collect();
    let Ok(mut rows) = stmt.query(rusqlite::params_from_iter(bound_refs)) else {
        return Ok(None);
    };

    let mut out: Vec<u8> = vec![b'['];
    loop {
        let row = match rows.next() {
            Ok(Some(row)) => row,
            Ok(None) => break,
            Err(error) if is_too_big(&error) => return Err(value_too_big(max_bytes)),
            Err(_) => break,
        };
        if out.len() > 1 {
            out.push(b',');
        }
        out.push(b'{');
        for (i, name) in col_names.iter().enumerate() {
            if i > 0 {
                out.push(b',');
            }
            write_json_str(&mut out, name);
            out.push(b':');
            match row.get_ref(i) {
                Ok(ValueRef::Integer(v)) => {
                    let _ = write!(out, "{v}");
                }
                Ok(ValueRef::Real(v)) => {
                    let _ = serde_json::to_writer(&mut out, &v);
                }
                Ok(ValueRef::Text(bytes)) => {
                    ensure_room(&out, bytes.len(), max_bytes)?;
                    write_json_str(&mut out, &String::from_utf8_lossy(bytes));
                }
                Ok(ValueRef::Blob(bytes)) => {
                    ensure_room(&out, bytes.len().div_ceil(3) * 4, max_bytes)?;
                    use base64::Engine;
                    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                    write_json_str(&mut out, &encoded);
                }
                Ok(ValueRef::Null) | Err(_) => out.extend_from_slice(b"null"),
            }
            ensure_room(&out, 0, max_bytes)?;
        }
        out.push(b'}');
    }
    out.push(b']');
    ensure_room(&out, 0, max_bytes)?;
    Ok(String::from_utf8(out).ok())
}

fn bind_param(v: serde_json::Value) -> Box<dyn rusqlite::ToSql> {
    match v {
        serde_json::Value::Null => Box::new(rusqlite::types::Null),
        serde_json::Value::Bool(b) => Box::new(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Box::new(i)
            } else if let Some(f) = n.as_f64() {
                Box::new(f)
            } else {
                Box::new(n.to_string())
            }
        }
        serde_json::Value::String(s) => Box::new(s),
        other => Box::new(other.to_string()),
    }
}

fn write_json_str(out: &mut Vec<u8>, s: &str) {
    let _ = serde_json::to_writer(out, s);
}

fn ensure_room(out: &[u8], incoming: usize, max_bytes: usize) -> Result<(), String> {
    if out.len().saturating_add(incoming) > max_bytes {
        return Err(format!(
            "sqliteQuery result exceeds {max_bytes} bytes; select narrower columns or add a LIMIT"
        ));
    }
    Ok(())
}

fn is_too_big(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(e, _) if e.code == rusqlite::ErrorCode::TooBig
    )
}

fn value_too_big(max_bytes: usize) -> String {
    format!(
        "sqliteQuery touched a value over {max_bytes} bytes; filter large columns with octet_length() before reading them"
    )
}

#[cfg(test)]
mod tests;
