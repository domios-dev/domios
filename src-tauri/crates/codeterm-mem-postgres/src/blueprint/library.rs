use super::{calls, index, metadata, SOURCE};
use crate::{db_error, items};
use codeterm_blueprint_types::blueprint_memory::{self as model, Entry, MERGE_BUDGET};
use codeterm_mem::{
    backend::{blueprint::BlueprintChanges, WriteContext},
    blueprint_library::{
        merge_policy,
        qualification::{self, PreparedBlueprint},
        SaveResult,
    },
    embedder::ActiveEmbedder,
    solution_metadata::SolutionMetadata,
    store::MemUpdate,
    MemKind, MemScope, NewMemItem,
};
use postgres::Transaction;

pub(super) fn save(
    tx: &mut Transaction<'_>,
    ctx: &WriteContext,
    area: &str,
    prepared: PreparedBlueprint,
    merge: bool,
    area_cap: usize,
    purpose: Option<merge_policy::Purpose>,
    embedder: &ActiveEmbedder,
) -> Result<SaveResult, String> {
    let PreparedBlueprint {
        document,
        intents,
        evidence,
        team,
        index: mut index_row,
    } = prepared;
    let resolver = team
        .as_deref()
        .map(|team| calls::ScopedResolver::new(tx, area, team));
    let document = codeterm_mem::blueprint_library::call_revisions::freeze(
        resolver
            .as_ref()
            .map(|r| r as &dyn codeterm_blueprint_types::blueprint_document::calls::MemoryResolver),
        document,
    )?;
    drop(resolver);
    if !model::safe_template(&document, &intents.join("; ")) {
        return Err("Unsafe callee closure cannot be saved".into());
    }
    if team.is_some() {
        index_row = qualification::index_row(&document, &intents);
    }
    let yaml = document.yaml()?;
    let mut library = load_entries(tx, ctx, area, !evidence.kept, None)?;
    let mut scoped = Vec::new();
    for entry in library {
        if calls::same_scope(tx, &entry.id, area, team.as_deref())? {
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
                &index::read_vectors(tx, &entry.id, &embedder.identity())?,
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
        let mut record = metadata::read(tx, &existing.id)?.ok_or("Blueprint metadata absent")?;
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
        let stored = index::read_row(tx, &existing.id)?;
        if evidence.kept {
            next.document = candidate.document;
            next.intents = candidate.intents;
            index_row.enrichment = stored.and_then(|row| row.enrichment);
            index::write_row(tx, &existing.id, &index_row)?;
        } else if stored.is_none() {
            index::write_row(tx, &existing.id, &index_row)?;
        }
        next.id = existing.id.clone();
        record.observe(&evidence, now.timestamp());
        if let Some(team) = &team {
            calls::scope(tx, &next.id, area, team)?;
        }
        write_entry(tx, ctx, &next, record)?;
        let id = next.id.clone();
        library.retain(|entry| entry.id != id);
        library.push(next);
        id
    } else {
        let id = items::insert_in(
            tx,
            ctx,
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
            None,
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
            calls::scope(tx, &next.id, area, team)?;
        }
        write_entry(tx, ctx, &next, record)?;
        index::write_row(tx, &id, &index_row)?;
        library.push(next);
        id
    };
    if !evidence.tags.is_empty() {
        let mut tags = items::get_in(tx, &id)?
            .ok_or("Blueprint solution disappeared")?
            .tags;
        tags.retain(|tag| !codeterm_blueprint_types::blueprint_tags::contains(tag));
        tags.extend(evidence.tags);
        items::update_in(
            tx,
            ctx,
            &id,
            &MemUpdate {
                tags: Some(tags),
                ..Default::default()
            },
        )?;
    }
    let evicted_ids = model::evictions(&library, area_cap, now.timestamp(), &id);
    for evicted in &evicted_ids {
        archive(tx, ctx, evicted, "blueprint solution area cap")?;
    }

    Ok(SaveResult { id, evicted_ids })
}

pub(super) fn archive(
    tx: &mut Transaction<'_>,
    ctx: &WriteContext,
    id: &str,
    reason: &str,
) -> Result<(), String> {
    crate::curation::archive_in(
        tx,
        ctx,
        id,
        "system:blueprint-library",
        "archive",
        Some(reason),
    )?;
    Ok(())
}

fn write_entry(
    tx: &mut Transaction<'_>,
    ctx: &WriteContext,
    entry: &Entry,
    mut record: SolutionMetadata,
) -> Result<(), String> {
    record.set_entry(entry);
    let document = &entry.document;
    let mut tags = items::get_in(tx, &entry.id)?
        .map(|item| item.tags)
        .unwrap_or_default();
    tags.retain(|tag| codeterm_blueprint_types::blueprint_tags::contains(tag));
    tags.push("blueprint-library".into());
    if let Some(curator) = &record.curator {
        tags.push("curator".into());
        tags.extend(curator.rounds.keys().cloned());
    }
    items::update_in(
        tx,
        ctx,
        &entry.id,
        &MemUpdate {
            title: Some(document.name.clone()),
            body: Some(document.yaml()?),
            summary: Some(entry.when_to_use()),
            tags: Some(tags),
            ..Default::default()
        },
    )?;
    calls::retain(tx, &entry.id, document)?;
    metadata::write(tx, &entry.id, &record)
}

fn entry(tx: &mut Transaction<'_>, id: String, validate: bool) -> Result<Entry, String> {
    let item = items::get_in(tx, &id)?.ok_or("Blueprint solution disappeared")?;
    let stats = metadata::read(tx, &id)?
        .and_then(|record| record.blueprint)
        .ok_or("Blueprint metadata absent")?;
    let document = model::parse_document(&item.body)?;
    if validate {
        let errors = document.template_errors();
        if !errors.is_empty() {
            return Err(format!("Saved solution {id} is unavailable: {errors:?}"));
        }
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
}

pub(super) fn load_entries(
    tx: &mut Transaction<'_>,
    ctx: &WriteContext,
    area: &str,
    validate: bool,
    team: Option<&str>,
) -> Result<Vec<Entry>, String> {
    let rows=tx.query("SELECT m.id FROM mem_items m JOIN solution_metadata s ON s.item_id=m.id WHERE m.kind='skill' AND ($1='' OR m.area=$1) AND m.source=$2 ORDER BY (s.metadata_json::jsonb #>> '{blueprint,successes}')::numeric DESC NULLS LAST,(s.metadata_json::jsonb #>> '{blueprint,last_used}')::numeric DESC NULLS LAST,m.id", &[&area,&SOURCE]).map_err(db_error)?;
    let mut result = Vec::new();
    for row in rows {
        let id: String = row.get(0);
        if team.is_some() && !calls::same_scope(tx, &id, area, team)? {
            continue;
        }
        let item = items::get_in(tx, &id)?.ok_or("Blueprint solution disappeared")?;
        metadata::read(tx, &id)?
            .and_then(|record| record.blueprint)
            .ok_or("Blueprint metadata absent")?;
        match model::parse_document(&item.body) {
            Ok(document) if !validate || document.template_errors().is_empty() => {
                result.push(entry(tx, id, false)?)
            }
            _ => archive(tx, ctx, &id, "Blueprint recipe fails current validation")?,
        }
    }
    Ok(result)
}

pub(super) fn readonly(tx: &mut Transaction<'_>, area: &str) -> Result<Vec<Entry>, String> {
    let rows=tx.query("SELECT m.id FROM mem_items m JOIN solution_metadata s ON s.item_id=m.id WHERE m.kind='skill' AND m.area=$1 AND m.source=$2 ORDER BY m.id", &[&area,&SOURCE]).map_err(db_error)?;
    rows.into_iter()
        .map(|row| entry(tx, row.get(0), true))
        .collect()
}

pub(super) fn reuse(
    tx: &mut Transaction<'_>,
    _ctx: &WriteContext,
    ids: &[String],
) -> Result<(), String> {
    for id in ids.iter().take(5) {
        let Some(mut record) = metadata::read(tx, id)? else {
            continue;
        };
        let stats = record
            .blueprint
            .as_mut()
            .ok_or("Blueprint metadata absent")?;
        stats.uses = stats.uses.saturating_add(1);
        stats.last_used = chrono::Utc::now().timestamp();
        metadata::write(tx, id, &record)?;
    }
    Ok(())
}

pub(super) fn dream(
    tx: &mut Transaction<'_>,
    ctx: &WriteContext,
    embedder: &ActiveEmbedder,
    turn: u64,
) -> Result<BlueprintChanges, String> {
    let count: i64 = tx
        .query_one(
            "SELECT COUNT(DISTINCT area) FROM mem_items WHERE kind='skill' AND source=$1",
            &[&SOURCE],
        )
        .map_err(db_error)?
        .get(0);
    let offset = (turn % (count.max(1) as u64)) as i64;
    let areas=tx.query("SELECT area FROM mem_items WHERE kind='skill' AND source=$1 GROUP BY area ORDER BY area LIMIT 1 OFFSET $2", &[&SOURCE,&offset]).map_err(db_error)?;
    let mut changed = Vec::new();
    let mut changed_areas = std::collections::BTreeSet::new();
    for area in areas {
        let area: String = area.get(0);
        let library = load_entries(tx, ctx, &area, true, None)?;
        for merge in model::dream_merges(&library, MERGE_BUDGET) {
            if !calls::merge_allowed(tx, &merge.keeper.id, &merge.superseded)? {
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
                    &index::read_vectors(tx, &first.id, &identity)?,
                    &identity,
                    first,
                )?
                .agrees(&merge_policy::Purpose::stored(
                    &index::read_vectors(tx, &second.id, &identity)?,
                    &identity,
                    second,
                )?) || embedder.identity() != identity
                {
                    continue;
                }
            }
            let mut record =
                metadata::read(tx, &merge.keeper.id)?.ok_or("Blueprint metadata absent")?;
            record
                .merge(metadata::read(tx, &merge.superseded)?.ok_or("Blueprint metadata absent")?);
            write_entry(tx, ctx, &merge.keeper, record)?;
            archive(
                tx,
                ctx,
                &merge.superseded,
                &format!("blueprint solution merged into {}", merge.keeper.id),
            )?;
            changed.push(merge.keeper.id);
            changed_areas.insert(area.clone());
        }
    }
    Ok(BlueprintChanges {
        ids: changed,
        areas: changed_areas.into_iter().collect(),
    })
}
