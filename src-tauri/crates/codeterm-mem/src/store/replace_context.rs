use super::{NewMemItem, ReplaceGuard, ReplaceGuardError};
use rusqlite::OptionalExtension;

#[derive(Debug)]
pub enum GuardedSaveError {
    Guard(ReplaceGuardError),
    Store(String),
}

impl From<String> for GuardedSaveError {
    fn from(error: String) -> Self {
        Self::Store(error)
    }
}

impl std::fmt::Display for GuardedSaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Guard(_) => f.write_str("Guarded memory save refused"),
            Self::Store(message) => f.write_str(message),
        }
    }
}

thread_local! {
    static GUARD: std::cell::RefCell<Option<ReplaceGuard>> = const { std::cell::RefCell::new(None) };
}

pub fn with_replace_guard<R>(guard: ReplaceGuard, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<ReplaceGuard>);
    impl Drop for Restore {
        fn drop(&mut self) {
            GUARD.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(GUARD.with(|slot| slot.replace(Some(guard))));
    f()
}

pub fn current_replace_guard() -> Option<ReplaceGuard> {
    GUARD.with(|slot| slot.borrow().clone())
}

pub(crate) fn check_guarded_save(
    conn: &rusqlite::Connection,
    item: &NewMemItem,
) -> Result<(), GuardedSaveError> {
    let Some(guard) = current_replace_guard() else {
        return Ok(());
    };
    let map = GuardedSaveError::Guard;
    guard
        .check_insert(item.area.as_deref().unwrap_or_default())
        .map_err(map)?;
    let existing = conn
        .query_row(
            "SELECT area, is_pinned_wakeup FROM mem_items WHERE kind=?1 AND title=?2 LIMIT 1",
            rusqlite::params![item.kind.as_str(), item.title],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, bool>(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some((area, pinned)) = existing {
        guard
            .check_replace(area.as_deref().unwrap_or_default(), item.kind, pinned)
            .map_err(map)?;
    }
    Ok(())
}
