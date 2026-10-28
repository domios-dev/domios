use super::*;

pub struct PreparedBlueprint {
    pub document: Document,
    pub team: Option<String>,
    pub intents: Vec<String>,
    pub evidence: Evidence,
    pub index: index::IndexRow,
}

pub(crate) fn index_row(original: &Document, intents: &[String]) -> index::IndexRow {
    use codeterm_blueprint_types::blueprint_memory::semantic_canon::{
        canonical_json, semantic_canon,
    };
    use sha2::{Digest, Sha256};
    index::IndexRow {
        document_hash: Sha256::digest(canonical_json(original).as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        canon: semantic_canon(original, intents.first().map_or("", String::as_str)),
        gists: semantic_canon(original, ""),
        signature: crate::blueprint_wl::signature(original),
        enrichment: None,
    }
}

impl MemManager {
    pub(crate) fn qualify_blueprint(
        &self,
        area: &str,
        document: Document,
        intents: &[String],
        mut evidence: Evidence,
    ) -> Result<PreparedBlueprint, String> {
        if document.trigger.is_none()
            && document.result.port.is_some()
            && codeterm_blueprint_types::blueprint_document::calls::has_unsealed_code_files(
                &document,
            )
        {
            return Err(
                "Callable code files must be frozen into inline source bytes before saving".into(),
            );
        }
        if !evidence.tags.is_empty() {
            codeterm_blueprint_types::blueprint_tags::validate(&evidence.tags)
                .map_err(|error| error.to_string())?;
        }
        let intents = model::validate_intents(intents)?;
        let joined = intents.join("; ");
        let when_to_use = joined.as_str();
        let index = index_row(&document, &intents);
        let document = if document.trigger.is_none() && document.result.port.is_some() {
            document
        } else {
            model::abstract_document(document)
        };
        if !document.template_errors().is_empty() {
            return Err("Blueprint solution fails current template validation".into());
        }
        evidence.safe = model::safe_template(&document, when_to_use);
        let decision = model::decide(&evidence);
        if !decision.save {
            return Err(format!("Blueprint solution refused: {}", decision.reason));
        }
        if let Some(curator) = &evidence.curator {
            let relations = self
                .recall_solutions(
                    &crate::SearchQuery {
                        query: when_to_use.into(),
                        kind: Some("skill".into()),
                        area: Some(area.into()),
                        limit: Some(3),
                        ..Default::default()
                    },
                    Some(SOURCE),
                )?
                .into_iter()
                .map(|hit| {
                    let similarity = crate::solution_search::intent_similarity(
                        when_to_use,
                        hit.item.summary.as_deref().unwrap_or_default(),
                    );
                    let covers = model::parse_document(&hit.item.body).is_ok_and(|existing| {
                        model::structure(&existing) == model::structure(&document)
                    });
                    crate::solution_search::relation(hit.item.id, similarity, covers)
                })
                .collect::<Vec<_>>();
            model::curator::decide(
                &curator.episodes.iter().cloned().collect(),
                curator.granted,
                false,
                &relations,
            )?;
        }
        if !model::safe_template(&document, when_to_use) {
            return Err("Unsafe Blueprint evidence cannot be saved".into());
        }
        if document.yaml()?.len() > codeterm_blueprint_types::blueprint_recipe::MAX_RECIPE_BYTES {
            return Err("Blueprint solution exceeds document limit".into());
        }
        Ok(PreparedBlueprint {
            team: None,
            document,
            intents,
            evidence,
            index,
        })
    }
}
