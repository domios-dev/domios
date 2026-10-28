use super::{calls, index, metadata, SqliteMemStore, WriteContext, SOURCE};
use crate::blueprint_library::{
    merge_policy,
    qualification::{self, PreparedBlueprint},
    SaveResult,
};
use crate::{solution_metadata::SolutionMetadata, store, MemKind, MemScope, NewMemItem};
use codeterm_blueprint_types::blueprint_memory::{self as model, Entry, MERGE_BUDGET};
use rusqlite::{params, Connection};
pub(super) fn load_entries(
    ctx: &WriteContext,
    conn: &Connection,
    blobs: &std::path::Path,
    area: &str,
    validate: bool,
) -> Result<Vec<Entry>, String> {
    load_entries_in(ctx, conn, blobs, area, validate)
}

pub(crate) fn load_entries_in(
    ctx: &WriteContext,
    conn: &Connection,
    blobs: &std::path::Path,
    area: &str,
    validate: bool,
) -> Result<Vec<Entry>, String> {
    load_entries_scoped(ctx, conn, blobs, area, validate, None)
}

pub(super) fn load_entries_scoped(
    ctx: &WriteContext,
    conn: &Connection,
    blobs: &std::path::Path,
    area: &str,
    validate: bool,
    team: Option<&str>,
) -> Result<Vec<Entry>, String> {
    let mut stmt=conn.prepare("SELECT m.id FROM mem_items m JOIN solution_metadata s ON s.item_id=m.id WHERE m.kind='skill' AND (?1='' OR m.area=?1) AND m.source=?2 ORDER BY json_extract(s.metadata_json,'$.blueprint.successes') DESC, json_extract(s.metadata_json,'$.blueprint.last_used') DESC, m.id").map_err(|error|error.to_string())?;
    let ids = stmt
        .query_map(params![area, SOURCE], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut result = Vec::new();
    for id in ids {
        if team.is_some() && !calls::same_scope(conn, &id, area, team)? {
            continue;
        }
        let item = store::get(conn, &id, blobs)?.ok_or("Blueprint solution disappeared")?;
        let stats = metadata::read(conn, &id)?
            .and_then(|record| record.blueprint)
            .ok_or("Blueprint metadata absent")?;
        let document = match model::parse_document(&item.body) {
            Ok(document) if !validate || document.template_errors().is_empty() => document,
            _ => {
                archive(ctx, conn, &id, "Blueprint recipe fails current validation")?;
                continue;
            }
        };
        result.push(Entry {
            id,
            document,
            intents: stats.intents,
            uses: stats.uses,
            successes: stats.successes,
            failures: stats.failures,
            kept: stats.explicitly_kept,
            last_used: stats.last_used,
        });
    }
    Ok(result)
}

fn entries(
    ctx: &WriteContext,
    conn: &Connection,
    blobs: &std::path::Path,
    area: &str,
) -> Result<Vec<Entry>, String> {
    load_entries(ctx, conn, blobs, area, true)
}

fn archive(ctx: &WriteContext, conn: &Connection, id: &str, reason: &str) -> Result<(), String> {
    crate::curation::calibration::archive::archive_item_with_context(
        ctx,
        conn,
        id,
        "skill",
        "system:blueprint-library",
        "archive",
        Some(reason),
    )
}

fn write_entry(
    ctx: &WriteContext,
    conn: &Connection,
    blobs: &std::path::Path,
    entry: &Entry,
    mut record: SolutionMetadata,
) -> Result<(), String> {
    {
        record.set_entry(entry);
        let document = &entry.document;
        let mut tags = store::get(conn, &entry.id, blobs)?
            .map(|item| item.tags)
            .unwrap_or_default();
        tags.retain(|tag| codeterm_blueprint_types::blueprint_tags::contains(tag));
        tags.push("blueprint-library".into());
        if let Some(curator) = &record.curator {
            tags.push("curator".into());
            tags.extend(curator.rounds.keys().cloned());
        }
        store::update_with_context(
            ctx,
            conn,
            &entry.id,
            &store::MemUpdate {
                title: Some(document.name.clone()),
                body: Some(document.yaml()?),
                summary: Some(entry.when_to_use()),
                tags: Some(tags),
                ..Default::default()
            },
            blobs,
        )?;
        calls::retain(conn, &entry.id, document)?;
        metadata::write(conn, &entry.id, &record)
    }
}

impl SqliteMemStore {
    pub(super) fn save_blueprint(
        &self,
        ctx: &WriteContext,
        area: &str,
        prepared: PreparedBlueprint,
        merge: bool,
        area_cap: usize,
        purpose: Option<merge_policy::Purpose>,
        embedder: &crate::embedder::ActiveEmbedder,
    ) -> Result<SaveResult, String> {
        {
            let PreparedBlueprint {
                document,
                intents,
                evidence,
                team,
                index: mut index_row,
            } = prepared;
            let conn = self.conn.lock();
            let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
            let resolver = team.as_deref().map(|team| calls::ScopedResolver {
                conn: &tx,
                blobs: &self.blobs_dir,
                area,
                team,
            });
            let document = crate::blueprint_library::call_revisions::freeze(
                resolver.as_ref().map(|r| {
                    r as &dyn codeterm_blueprint_types::blueprint_document::calls::MemoryResolver
                }),
                document,
            )?;
            if !model::safe_template(&document, &intents.join("; ")) {
                return Err("Unsafe callee closure cannot be saved".into());
            }
            if team.is_some() {
                index_row = qualification::index_row(&document, &intents);
            }
            let yaml = document.yaml()?;
            let mut library = load_entries(ctx, &tx, &self.blobs_dir, area, !evidence.kept)?;
            let mut scoped = Vec::new();
            for entry in library {
                if calls::same_scope(&tx, &entry.id, area, team.as_deref())? {
                    scoped.push(entry);
                }
            }
            library = scoped;
            merge_policy::check_name(&library, &document, &intents)?;
            if purpose
                .as_ref()
                .is_some_and(|purpose| embedder.identity() != purpose.identity)
            {
                return Err("Blueprint embedder changed during save".into());
            }
            let now = chrono::Utc::now();
            let mut compatible = Vec::new();
            for entry in &library {
                if let Some(purpose) = &purpose {
                    if !purpose.agrees(&merge_policy::Purpose::stored(
                        &index::read_vectors(&tx, &entry.id, &embedder.identity())?,
                        &embedder.identity(),
                        entry,
                    )?) {
                        continue;
                    }
                }
                compatible.push(entry.clone());
            }
            let existing = merge
                .then(|| model::duplicate(&compatible, &document))
                .flatten();
            let id = if let Some(existing) = existing {
                let mut record =
                    metadata::read(&tx, &existing.id)?.ok_or("Blueprint metadata absent")?;
                let prior = record.prior(evidence.origin.as_deref());
                let candidate = Entry {
                    id: "candidate".into(),
                    document,
                    intents: intents.clone(),
                    uses: 0,
                    successes: evidence.successes.saturating_sub(prior.successes),
                    failures: evidence.failures.saturating_sub(prior.failures),
                    kept: evidence.kept,
                    last_used: now.timestamp(),
                };
                let mut next = model::merge_pair(existing, &candidate)
                    .ok_or("Blueprint structure changed")?
                    .keeper;
                let stored = index::read_row(&tx, &existing.id)?;
                if evidence.kept {
                    next.document = candidate.document;
                    next.intents = candidate.intents;
                    index_row.enrichment = stored.and_then(|row| row.enrichment);
                    index::write_row(&tx, &existing.id, &index_row)?;
                } else if stored.is_none() {
                    index::write_row(&tx, &existing.id, &index_row)?;
                }
                next.id = existing.id.clone();
                record.observe(&evidence, now.timestamp());
                if let Some(team) = &team {
                    calls::scope(&tx, &next.id, area, team)?;
                }
                write_entry(ctx, &tx, &self.blobs_dir, &next, record)?;
                let id = next.id.clone();
                library.retain(|entry| entry.id != id);
                library.push(next);
                id
            } else {
                let id = store::insert_with_context(
                    ctx,
                    &tx,
                    &NewMemItem {
                        kind: MemKind::Skill,
                        scope: MemScope::Global,
                        title: format!("Blueprint solution {}", ulid::Ulid::new()),
                        summary: document.note.clone(),
                        area: Some(area.into()),
                        tags: vec!["blueprint-library".into()],
                        body: yaml,
                        source: Some(SOURCE.into()),
                        importance: 0.5,
                        source_path: None,
                        pinned: false,
                    },
                    &self.blobs_dir,
                )?;
                let next = Entry {
                    id: id.clone(),
                    document,
                    intents: intents.clone(),
                    uses: 0,
                    successes: evidence.successes,
                    failures: evidence.failures,
                    kept: evidence.kept,
                    last_used: now.timestamp(),
                };
                let mut record = SolutionMetadata::default();
                record.observe(&evidence, now.timestamp());
                if let Some(team) = &team {
                    calls::scope(&tx, &next.id, area, team)?;
                }
                write_entry(ctx, &tx, &self.blobs_dir, &next, record)?;
                index::write_row(&tx, &id, &index_row)?;
                library.push(next);
                id
            };
            if !evidence.tags.is_empty() {
                let mut tags = store::get(&tx, &id, &self.blobs_dir)?
                    .ok_or("Blueprint solution disappeared")?
                    .tags;
                tags.retain(|tag| !codeterm_blueprint_types::blueprint_tags::contains(tag));
                tags.extend(evidence.tags);
                store::update_with_context(
                    ctx,
                    &tx,
                    &id,
                    &store::MemUpdate {
                        tags: Some(tags),
                        ..Default::default()
                    },
                    &self.blobs_dir,
                )?;
            }
            let evicted_ids = model::evictions(&library, area_cap, now.timestamp(), &id);
            for evicted in &evicted_ids {
                archive(ctx, &tx, evicted, "blueprint solution area cap")?;
            }
            tx.commit().map_err(|e| e.to_string())?;
            Ok(SaveResult { id, evicted_ids })
        }
    }

    pub(super) fn reuse_blueprints(
        &self,
        _ctx: &WriteContext,
        ids: &[String],
    ) -> Result<(), String> {
        let conn = self.conn.lock();
        let tx = conn
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        for id in ids.iter().take(5) {
            let Some(mut record) = metadata::read(&tx, id)? else {
                continue;
            };
            let stats = record
                .blueprint
                .as_mut()
                .ok_or("Blueprint metadata absent")?;
            stats.uses = stats.uses.saturating_add(1);
            stats.last_used = chrono::Utc::now().timestamp();
            metadata::write(&tx, id, &record)?;
        }
        tx.commit().map_err(|error| error.to_string())
    }

    pub(super) fn blueprint_solutions_readonly(&self, area: &str) -> Result<Vec<Entry>, String> {
        let snapshots = {
            let conn = self.conn.lock();
            let mut stmt = conn.prepare("SELECT m.id FROM mem_items m JOIN solution_metadata s ON s.item_id=m.id WHERE m.kind='skill' AND m.area=?1 AND m.source=?2 ORDER BY m.id").map_err(|e| e.to_string())?;
            let ids = stmt
                .query_map(params![area, SOURCE], |row| row.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            ids.into_iter()
                .map(|id| {
                    let item = store::get(&conn, &id, &self.blobs_dir)?
                        .ok_or("Blueprint solution disappeared")?;
                    let stats = metadata::read(&conn, &id)?
                        .and_then(|record| record.blueprint)
                        .ok_or("Blueprint metadata absent")?;
                    Ok((id, item.body, stats))
                })
                .collect::<Result<Vec<_>, String>>()?
        };
        snapshots
            .into_iter()
            .map(|(id, body, stats)| {
                let document = model::parse_document(&body)?;
                let errors = document.template_errors();
                if !errors.is_empty() {
                    return Err(format!("Saved solution {id} is unavailable: {errors:?}"));
                }
                Ok(Entry {
                    id,
                    document,
                    intents: stats.intents,
                    uses: stats.uses,
                    successes: stats.successes,
                    failures: stats.failures,
                    kept: stats.explicitly_kept,
                    last_used: stats.last_used,
                })
            })
            .collect()
    }

    pub(super) fn archive_blueprint_solution(
        &self,
        ctx: &WriteContext,
        area: &str,
        id: &str,
    ) -> Result<(), String> {
        let conn = self.conn.lock();
        let tx = conn
            .unchecked_transaction()
            .map_err(|error| error.to_string())?;
        if !entries(ctx, &tx, &self.blobs_dir, area)?
            .iter()
            .any(|entry| entry.id == id)
        {
            return Err("Saved Blueprint recipe not found in this project".into());
        }
        archive(ctx, &tx, id, "Archived by owner")?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(())
    }

    pub(super) fn dream_blueprints(
        &self,
        ctx: &WriteContext,
        embedder: &crate::embedder::ActiveEmbedder,
        turn: u64,
    ) -> Result<crate::backend::blueprint::BlueprintChanges, String> {
        let conn = self.conn.lock();
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let areas = {
            let count: u64 = tx
                .query_row(
                    "SELECT COUNT(DISTINCT area) FROM mem_items WHERE kind='skill' AND source=?1",
                    [SOURCE],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;
            let offset = turn % count.max(1);
            let mut stmt=tx.prepare("SELECT area FROM mem_items WHERE kind='skill' AND source=?1 GROUP BY area ORDER BY area LIMIT 1 OFFSET ?2").map_err(|e|e.to_string())?;
            let rows = stmt
                .query_map(params![SOURCE, offset], |row| row.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            rows
        };
        let mut changed = Vec::new();
        let mut changed_areas = std::collections::BTreeSet::new();
        for area in areas {
            let library = entries(ctx, &tx, &self.blobs_dir, &area)?;
            for entry in &library {
                if entry.successes >= 10 && entry.uses >= 5 {
                    log::debug!(
                        "[blueprint-memory] built-in promotion proposal: {}",
                        entry.id
                    );
                }
            }
            for merge in model::dream_merges(&library, MERGE_BUDGET) {
                if !calls::merge_allowed(&tx, &merge.keeper.id, &merge.superseded)? {
                    continue;
                }
                if merge_policy::callable(&merge.keeper.document) {
                    let identity = embedder.identity();
                    let first = library
                        .iter()
                        .find(|entry| entry.id == merge.keeper.id)
                        .ok_or("Blueprint keeper missing")?;
                    let second = library
                        .iter()
                        .find(|entry| entry.id == merge.superseded)
                        .ok_or("Blueprint duplicate missing")?;
                    if !merge_policy::Purpose::stored(
                        &index::read_vectors(&tx, &first.id, &identity)?,
                        &identity,
                        first,
                    )?
                    .agrees(&merge_policy::Purpose::stored(
                        &index::read_vectors(&tx, &second.id, &identity)?,
                        &identity,
                        second,
                    )?) || embedder.identity() != identity
                    {
                        continue;
                    }
                }
                let mut record =
                    metadata::read(&tx, &merge.keeper.id)?.ok_or("Blueprint metadata absent")?;
                record.merge(
                    metadata::read(&tx, &merge.superseded)?.ok_or("Blueprint metadata absent")?,
                );
                write_entry(ctx, &tx, &self.blobs_dir, &merge.keeper, record)?;
                archive(
                    ctx,
                    &tx,
                    &merge.superseded,
                    &format!("blueprint solution merged into {}", merge.keeper.id),
                )?;
                changed.push(merge.keeper.id);
                changed_areas.insert(area.clone());
            }
        }
        tx.commit().map_err(|e| e.to_string())?;
        Ok(crate::backend::blueprint::BlueprintChanges {
            ids: changed,
            areas: changed_areas.into_iter().collect(),
        })
    }
}
