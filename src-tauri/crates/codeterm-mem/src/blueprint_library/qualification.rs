use super::*;

pub(crate) struct PreparedBlueprint {
    pub(super) document: Document,
    pub(super) when_to_use: String,
    pub(super) evidence: Evidence,
}

impl MemManager {
    pub(super) fn qualify_blueprint(
        &self,
        area: &str,
        document: Document,
        when_to_use: &str,
        mut evidence: Evidence,
    ) -> Result<PreparedBlueprint, String> {
        if when_to_use.trim().is_empty()
            || when_to_use.chars().count() > 200
            || when_to_use.chars().any(char::is_control)
        {
            return Err("When to use must contain 1..200 plain-text characters".into());
        }
        let document = model::abstract_document(document);
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
                .search(&crate::SearchQuery {
                    query: when_to_use.into(),
                    limit: Some(3),
                    ..Default::default()
                })?
                .into_iter()
                .map(|hit| {
                    if hit.item.source.as_deref() == Some(SOURCE)
                        && hit.item.area.as_deref() == Some(area)
                        && model::parse_document(&hit.item.body).is_ok_and(|existing| {
                            model::structure(&existing) == model::structure(&document)
                        })
                    {
                        model::curator::Relation::Covers(hit.item.id)
                    } else {
                        model::curator::Relation::Conflict(hit.item.id)
                    }
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
            document,
            when_to_use: when_to_use.into(),
            evidence,
        })
    }
}
