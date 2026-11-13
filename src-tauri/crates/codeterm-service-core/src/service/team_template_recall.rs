//! Template recall on the existing mem store, the way the Blueprint library does it: one
//! `document` item per template name, written by a system actor, keyed by its `source`, and
//! archived when the template leaves. General and the session dialog both query here.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};

use codeterm_team_package::contracts::{
    TemplateProvenance, TemplateRecall, TemplateRef, TemplateSummary,
};
use codeterm_team_package::Package;
use serde_json::Value;

use super::team_exchange::{self, library::TemplateLibrary, ExchangeError};
use super::ServiceContext;
use crate::mem::backend::WriteContext;
use crate::mem::{MemKind, MemManager, MemScope, NewMemItem, SearchQuery};

pub const ACTOR: &str = "system:team-templates";
pub const TAG: &str = "team-template";
const KIND: &str = "document";
const SOURCE_PREFIX: &str = "team-template:";
const RECALL_LIMIT: usize = 20;

static INDEXED: AtomicBool = AtomicBool::new(false);

fn source(name: &str) -> String {
    format!("{SOURCE_PREFIX}{name}")
}

/// The mem item a template is recalled by: title, description and `when`, nothing private.
pub fn item(package: &Package, provenance: TemplateProvenance) -> NewMemItem {
    let document = &package.document;
    let title = document
        .title
        .clone()
        .unwrap_or_else(|| document.name.clone());
    let mut body = format!("{title}\n\n{}", document.description);
    if !document.when.is_empty() {
        body.push_str("\n\nUse when:\n");
        for when in &document.when {
            body.push_str(&format!("- {when}\n"));
        }
    }
    let provenance = match provenance {
        TemplateProvenance::Builtin => "builtin",
        TemplateProvenance::Saved => "saved",
        TemplateProvenance::Imported => "imported",
    };
    NewMemItem {
        kind: MemKind::Document,
        scope: MemScope::Global,
        title: format!("Team template {}", document.name),
        summary: Some(if document.when.is_empty() {
            document.description.clone()
        } else {
            document.when.join("; ")
        }),
        area: None,
        tags: vec![TAG.to_string(), provenance.to_string()],
        body,
        source: Some(source(&document.name)),
        importance: 0.5,
        source_path: None,
        pinned: false,
    }
}

/// The current package for a name: builtin first, then the newest saved version.
fn current(library: &TemplateLibrary, name: &str) -> Option<(Package, TemplateProvenance)> {
    team_exchange::resolve_in(
        library,
        &TemplateRef {
            name: name.to_string(),
            version: None,
        },
    )
}

fn indexed_ids(mem: &MemManager, name: Option<&str>) -> Result<Vec<(String, String)>, String> {
    let prefix = match name {
        Some(name) => source(name),
        None => SOURCE_PREFIX.to_string(),
    };
    Ok(mem
        .list_by_source_prefix(KIND, &prefix)?
        .into_iter()
        .filter_map(|item| {
            let indexed = item.source?.strip_prefix(SOURCE_PREFIX)?.to_string();
            (name.is_none() || name == Some(indexed.as_str())).then_some((item.id, indexed))
        })
        .collect())
}

fn archive(mem: &MemManager, id: &str) -> Result<(), String> {
    mem.archive(id, "team template removed", ACTOR)
        .map(|_| ())
        .map_err(|error| format!("{error:?}"))
}

/// Brings one name's recall item up to date: saved or replaced in place, or archived.
pub fn index_in(mem: &MemManager, library: &TemplateLibrary, name: &str) -> Result<(), String> {
    match current(library, name) {
        Some((package, provenance)) => mem
            .save_with_context(
                item(&package, provenance),
                true,
                WriteContext::system(ACTOR),
            )
            .map(|_| ())
            .map_err(|error| error.to_string()),
        None => {
            for (id, _) in indexed_ids(mem, Some(name))? {
                archive(mem, &id)?;
            }
            Ok(())
        }
    }
}

/// Indexes every builtin and saved template and archives entries whose template is gone.
pub fn reindex_in(mem: &MemManager, library: &TemplateLibrary) -> Result<(), String> {
    let mut names: BTreeSet<String> = team_exchange::builtin_packages()
        .iter()
        .map(|package| package.document.name.clone())
        .collect();
    names.extend(library.list().into_iter().map(|summary| summary.name));
    for (id, indexed) in indexed_ids(mem, None)? {
        if !names.contains(&indexed) {
            archive(mem, &id)?;
        }
    }
    for name in &names {
        index_in(mem, library, name)?;
    }
    Ok(())
}

/// Re-indexes one template after it was saved, updated, imported or deleted.
pub fn index_template(svc: &ServiceContext, name: &str) -> Result<(), String> {
    index_in(&svc.mem, TemplateLibrary::global(), name)
}

/// Indexes builtins and My templates; startup and the first recall call it.
pub fn reindex(svc: &ServiceContext) -> Result<(), String> {
    reindex_in(&svc.mem, TemplateLibrary::global())?;
    INDEXED.store(true, Ordering::Release);
    Ok(())
}

/// Recalled names, best match first, from mem hits.
pub fn recalled_names(hits: &[crate::mem::SearchHit]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    hits.iter()
        .filter_map(|hit| hit.item.source.as_deref()?.strip_prefix(SOURCE_PREFIX))
        .filter(|name| seen.insert(name.to_string()))
        .map(str::to_string)
        .collect()
}

fn summary(library: &TemplateLibrary, name: &str) -> Option<TemplateSummary> {
    current(library, name).map(|(package, provenance)| library.summarize(&package, provenance))
}

/// Best matches first from mem; the builtin default is always the fallback.
pub fn recall_in(
    mem: &MemManager,
    library: &TemplateLibrary,
    query: &str,
) -> Result<TemplateRecall, ExchangeError> {
    let fallback = summary(library, team_exchange::DEFAULT_TEMPLATE)
        .ok_or_else(|| ExchangeError::invalid("the builtin default template is missing"))?;
    let query = query.trim();
    let (matches, recall_available) = if query.is_empty() {
        (Vec::new(), true)
    } else {
        match mem.search(&SearchQuery {
            query: query.to_string(),
            kind: Some(KIND.to_string()),
            limit: Some(RECALL_LIMIT),
            ..Default::default()
        }) {
            Ok(hits) => (
                recalled_names(&hits)
                    .iter()
                    .filter_map(|name| summary(library, name))
                    .collect(),
                true,
            ),
            Err(error) => {
                log::warn!("[team-templates] recall search failed: {error}");
                (Vec::new(), false)
            }
        }
    };
    Ok(TemplateRecall {
        matches,
        fallback,
        recall_available,
    })
}

#[derive(serde::Deserialize)]
struct RecallParams {
    #[serde(default)]
    query: String,
}

/// `team.template.recall`: best matches first; the builtin default is always offered.
pub fn recall(svc: &ServiceContext, params: Option<&Value>) -> Result<Value, ExchangeError> {
    let request: RecallParams = serde_json::from_value(params.cloned().unwrap_or(Value::Null))
        .map_err(|error| ExchangeError::invalid(error.to_string()))?;
    if !INDEXED.load(Ordering::Acquire) {
        if let Err(error) = reindex(svc) {
            log::warn!("[team-templates] recall index refresh failed: {error}");
        }
    }
    let recalled = recall_in(&svc.mem, TemplateLibrary::global(), &request.query)?;
    serde_json::to_value(recalled).map_err(|error| ExchangeError::invalid(error.to_string()))
}

#[cfg(test)]
#[path = "team_template_recall/tests.rs"]
mod tests;
