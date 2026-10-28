use super::{SqliteMemStore, WriteContext};
use crate::backend::SaveError;
use crate::{manager_crud::SaveOutcome, store, NewMemItem};
use rusqlite::OptionalExtension;

impl SqliteMemStore {
    pub(super) fn save_item(
        &self,
        ctx: &WriteContext,
        item: &NewMemItem,
        replace: bool,
    ) -> Result<SaveOutcome, SaveError> {
        let conn = self.conn.lock();
        if ctx.replace_guard.is_some() {
            let tx = conn
                .unchecked_transaction()
                .map_err(|e| SaveError::Storage(e.to_string()))?;
            let outcome = self.save_in(ctx, &tx, item, replace)?;
            tx.commit().map_err(|e| SaveError::Storage(e.to_string()))?;
            Ok(outcome)
        } else {
            self.save_in(ctx, &conn, item, replace)
        }
    }

    fn save_in(
        &self,
        ctx: &WriteContext,
        conn: &rusqlite::Connection,
        item: &NewMemItem,
        replace: bool,
    ) -> Result<SaveOutcome, SaveError> {
        if !item.title.is_empty() {
            let existing: Option<(String, Option<String>)> = conn
                .query_row(
                    "SELECT id, body_inline FROM mem_items WHERE kind = ?1 AND title = ?2 LIMIT 1",
                    rusqlite::params![item.kind.as_str(), &item.title],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(|error| format!("find existing mem item: {error}"))?;
            if let Some((existing_id, current_body)) = existing {
                if let Some(guard) = &ctx.replace_guard {
                    let (area, pinned): (Option<String>, bool) = conn
                        .query_row(
                            "SELECT area, is_pinned_wakeup!=0 FROM mem_items WHERE id=?1",
                            [&existing_id],
                            |r| Ok((r.get(0)?, r.get(1)?)),
                        )
                        .map_err(|e| SaveError::Storage(e.to_string()))?;
                    guard.check_replace(area.as_deref().unwrap_or(""), item.kind, pinned)?;
                }
                let current_body = current_body
                    .or_else(|| {
                        store::get(conn, &existing_id, &self.blobs_dir)
                            .ok()
                            .flatten()
                            .map(|item| item.body)
                    })
                    .unwrap_or_default();
                let normalize = |body: &str| body.split_whitespace().collect::<Vec<_>>().join(" ");
                if normalize(&current_body) == normalize(&item.body)
                    && store::item_author(conn, &existing_id)? == Some(ctx.author.clone())
                {
                    let _ = conn.execute(
                        "UPDATE mem_items SET recurrence_count = recurrence_count + 1, accessed_at = ?1 WHERE id = ?2",
                        rusqlite::params![chrono::Utc::now().to_rfc3339(), &existing_id],
                    );
                    return Ok(SaveOutcome {
                        id: existing_id,
                        outcome: "unchanged".into(),
                    });
                }
                store::replace_existing_with_context(
                    ctx,
                    conn,
                    &existing_id,
                    item,
                    &self.blobs_dir,
                )?;

                return Ok(SaveOutcome {
                    id: existing_id,
                    outcome: "updated".into(),
                });
            }
        }

        if let Some(guard) = &ctx.replace_guard {
            guard.check_insert(item.area.as_deref().unwrap_or(""))?;
            if replace {
                let candidate =
                    crate::ids::compose(item.kind.as_str(), &crate::ids::slugify(&item.title));
                let target: Option<(Option<String>, String, bool)> = conn
                    .query_row(
                        "SELECT area, kind, is_pinned_wakeup!=0 FROM mem_items WHERE id=?1",
                        [&candidate],
                        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                    )
                    .optional()
                    .map_err(|e| SaveError::Storage(e.to_string()))?;
                if let Some((area, kind, pinned)) = target {
                    let kind = store::MemKind::from_str(&kind)
                        .ok_or(SaveError::Guard(store::ReplaceGuardError::TitleConflict))?;
                    guard.check_replace(area.as_deref().unwrap_or(""), kind, pinned)?;
                }
            }
        }
        let id = if replace {
            if let Some(existing_id) =
                store::find_by_kind_title(conn, item.kind.as_str(), &item.title)?
            {
                store::replace_existing_with_context(
                    ctx,
                    conn,
                    &existing_id,
                    item,
                    &self.blobs_dir,
                )?;

                return Ok(SaveOutcome {
                    id: existing_id,
                    outcome: "updated".into(),
                });
            }
            store::upsert_with_context(ctx, conn, item, &self.blobs_dir)?
        } else {
            store::insert_with_context(ctx, conn, item, &self.blobs_dir)?
        };

        Ok(SaveOutcome {
            id,
            outcome: "created".into(),
        })
    }
}
