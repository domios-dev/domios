use rusqlite::{ffi, Connection, Error};
use std::{
    ffi::CStr,
    os::raw::{c_char, c_int, c_void},
};

pub(super) fn install(connection: &Connection) -> bool {
    if connection.pragma_update(None, "query_only", true).is_err() {
        return false;
    }
    // The callback is static and carries no borrowed state; it lives with this connection.
    unsafe {
        ffi::sqlite3_set_authorizer(connection.handle(), Some(authorize), std::ptr::null_mut())
            == ffi::SQLITE_OK
    }
}

pub(super) fn denied(error: &Error) -> bool {
    matches!(error, Error::SqliteFailure(error, _) if error.code == rusqlite::ErrorCode::AuthorizationForStatementDenied)
}

unsafe extern "C" fn authorize(
    _: *mut c_void,
    action: c_int,
    first: *const c_char,
    second: *const c_char,
    _: *const c_char,
    _: *const c_char,
) -> c_int {
    let allowed = match action {
        ffi::SQLITE_SELECT | ffi::SQLITE_READ | ffi::SQLITE_RECURSIVE => true,
        ffi::SQLITE_FUNCTION => unsafe { named(second, PURE_FUNCTIONS) },
        ffi::SQLITE_PRAGMA => unsafe {
            named(first, SCHEMA_PRAGMAS) || second.is_null() && named(first, READ_PRAGMAS)
        },
        _ => false,
    };
    if allowed {
        ffi::SQLITE_OK
    } else {
        ffi::SQLITE_DENY
    }
}

unsafe fn named(value: *const c_char, names: &[&[u8]]) -> bool {
    if value.is_null() {
        return false;
    }
    // SQLite supplies NUL-terminated callback arguments that remain live for this call.
    let value = unsafe { CStr::from_ptr(value) }.to_bytes();
    names.iter().any(|name| value.eq_ignore_ascii_case(name))
}

const SCHEMA_PRAGMAS: &[&[u8]] = &[
    b"table_info",
    b"table_xinfo",
    b"table_list",
    b"index_info",
    b"index_xinfo",
    b"index_list",
    b"foreign_key_list",
];
const READ_PRAGMAS: &[&[u8]] = &[
    b"busy_timeout",
    b"query_only",
    b"user_version",
    b"schema_version",
    b"data_version",
    b"database_list",
    b"compile_options",
    b"encoding",
    b"page_count",
    b"freelist_count",
    b"page_size",
];
const PURE_FUNCTIONS: &[&[u8]] = &[
    b"current_date",
    b"current_time",
    b"current_timestamp",
    b"json_each",
    b"json_tree",
    b"abs",
    b"changes",
    b"char",
    b"coalesce",
    b"concat",
    b"concat_ws",
    b"format",
    b"glob",
    b"hex",
    b"if",
    b"ifnull",
    b"iif",
    b"instr",
    b"last_insert_rowid",
    b"length",
    b"like",
    b"likelihood",
    b"likely",
    b"lower",
    b"ltrim",
    b"max",
    b"min",
    b"nullif",
    b"octet_length",
    b"printf",
    b"quote",
    b"random",
    b"randomblob",
    b"replace",
    b"round",
    b"rtrim",
    b"sign",
    b"soundex",
    b"sqlite_compileoption_get",
    b"sqlite_compileoption_used",
    b"sqlite_offset",
    b"sqlite_source_id",
    b"sqlite_version",
    b"substr",
    b"substring",
    b"total_changes",
    b"trim",
    b"typeof",
    b"unhex",
    b"unicode",
    b"unlikely",
    b"upper",
    b"zeroblob",
    b"date",
    b"datetime",
    b"julianday",
    b"strftime",
    b"time",
    b"timediff",
    b"unixepoch",
    b"avg",
    b"count",
    b"group_concat",
    b"string_agg",
    b"sum",
    b"total",
    b"row_number",
    b"rank",
    b"dense_rank",
    b"percent_rank",
    b"cume_dist",
    b"ntile",
    b"lag",
    b"lead",
    b"first_value",
    b"last_value",
    b"nth_value",
    b"json",
    b"jsonb",
    b"json_array",
    b"jsonb_array",
    b"json_array_length",
    b"json_error_position",
    b"json_extract",
    b"jsonb_extract",
    b"json_insert",
    b"jsonb_insert",
    b"json_object",
    b"jsonb_object",
    b"json_patch",
    b"jsonb_patch",
    b"json_pretty",
    b"json_remove",
    b"jsonb_remove",
    b"json_replace",
    b"jsonb_replace",
    b"json_set",
    b"jsonb_set",
    b"json_type",
    b"json_valid",
    b"json_quote",
    b"json_group_array",
    b"jsonb_group_array",
    b"json_group_object",
    b"jsonb_group_object",
    b"->",
    b"->>",
    b"acos",
    b"acosh",
    b"asin",
    b"asinh",
    b"atan",
    b"atan2",
    b"atanh",
    b"ceil",
    b"ceiling",
    b"cos",
    b"cosh",
    b"degrees",
    b"exp",
    b"floor",
    b"ln",
    b"log",
    b"log10",
    b"log2",
    b"mod",
    b"pi",
    b"pow",
    b"power",
    b"radians",
    b"sin",
    b"sinh",
    b"sqrt",
    b"tan",
    b"tanh",
    b"trunc",
];
