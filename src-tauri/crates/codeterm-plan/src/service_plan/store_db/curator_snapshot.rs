use super::db_error;
use crate::plan::CuratorRound;
use crate::service_plan::PlanError;
use rusqlite::Connection;

pub(super) fn register_functions(connection: &Connection) -> Result<(), PlanError> {
    use rusqlite::functions::FunctionFlags;
    let flags = FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC;
    connection
        .create_scalar_function("domios_curator_round", 1, flags, |context| {
            let rusqlite::types::ValueRef::Text(bytes) = context.get_raw(0) else {
                return Ok(false);
            };
            if bytes.len() > 16 * 1024 {
                return Ok(false);
            }
            Ok(serde_json::from_slice::<CuratorRound>(bytes)
                .ok()
                .is_some_and(|round| !round.digest_hash.is_empty()))
        })
        .map_err(db_error)?;
    connection
        .create_scalar_function("domios_curator_timestamp", 1, flags, |context| {
            let value = match context.get_raw(0) {
                rusqlite::types::ValueRef::Text(bytes) => serde_json::from_slice::<u64>(bytes)
                    .ok()
                    .map(|value| format!("{value:020}")),
                _ => None,
            };
            Ok(value)
        })
        .map_err(db_error)
}
