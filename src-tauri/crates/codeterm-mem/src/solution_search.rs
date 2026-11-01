use crate::{search, MemManager, SearchHit, SearchQuery};
use codeterm_blueprint_types::blueprint_memory::curator::Relation;
use std::collections::BTreeSet;

pub(crate) const RELEVANCE_FLOOR: f64 = 0.55;
const SAME_INTENT: f64 = 0.85;

impl MemManager {
    pub fn recall_solutions(
        &self,
        query: &SearchQuery,
        source: Option<&str>,
    ) -> Result<Vec<SearchHit>, String> {
        let embedding = query_embedding(&query.query);
        self.recall_solutions_with_embedding(query, source, embedding.as_ref())
    }

    pub(crate) fn recall_solutions_with_embedding(
        &self,
        query: &SearchQuery,
        source: Option<&str>,
        embedding: Option<&crate::embedder::StampedVector>,
    ) -> Result<Vec<SearchHit>, String> {
        let (hits, relevant_vectors) = self.store.solution_candidates(query, source, embedding)?;
        Ok(hits
            .into_iter()
            .filter(|hit| {
                relevant_vectors.contains(&hit.item.id)
                    || lexical_relevance(&query.query, &hit.item) >= RELEVANCE_FLOOR
            })
            .collect())
    }

    pub fn curator_skill_relations(
        &self,
        item: &crate::NewMemItem,
    ) -> Result<Vec<Relation>, String> {
        self.recall_solutions(
            &SearchQuery {
                query: item.title.clone(),
                kind: Some("skill".into()),
                area: item.area.clone(),
                limit: Some(3),
                ..Default::default()
            },
            item.source.as_deref(),
        )?
        .into_iter()
        .map(|hit| {
            let similarity = intent_similarity(&item.title, &hit.item.title);
            if similarity < RELEVANCE_FLOOR {
                return Ok(Relation::Unrelated);
            }
            let covers = hit.item.body == item.body
                || self.curator_skill_covers(&hit.item.id, &item.body)?;
            Ok(relation(hit.item.id, similarity, covers))
        })
        .collect()
    }
}

fn lexical_relevance(query: &str, item: &crate::MemItem) -> f64 {
    let keywords = search::extract_keywords(query);
    if keywords.is_empty() {
        return 0.0;
    }
    let text = format!(
        "{} {}",
        item.title,
        item.summary.as_deref().unwrap_or_default()
    )
    .to_lowercase();
    let words = text
        .split(|c: char| !c.is_alphanumeric())
        .collect::<BTreeSet<_>>();
    keywords
        .iter()
        .filter(|keyword| words.contains(keyword.as_str()))
        .count() as f64
        / keywords.len() as f64
}

pub(crate) fn relation(id: String, similarity: f64, covers: bool) -> Relation {
    if similarity < RELEVANCE_FLOOR {
        Relation::Unrelated
    } else if covers {
        Relation::Covers(id)
    } else if similarity >= SAME_INTENT {
        Relation::Conflict(id)
    } else {
        Relation::Unrelated
    }
}

pub(crate) fn intent_similarity(left: &str, right: &str) -> f64 {
    let words = |text: &str| {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_lowercase)
            .collect::<BTreeSet<_>>()
    };
    let left_words = words(left);
    let right_words = words(right);
    let union = left_words.union(&right_words).count();
    let lexical = if union == 0 {
        0.0
    } else {
        left_words.intersection(&right_words).count() as f64 / union as f64
    };
    if lexical == 1.0 {
        return lexical;
    }
    #[cfg(feature = "embeddings")]
    if let (Some(left), Some(right)) = (
        query_embedding(left).map(|stamped| stamped.vector),
        query_embedding(right).map(|stamped| stamped.vector),
    ) {
        let dot: f64 = left
            .iter()
            .zip(&right)
            .map(|(a, b)| f64::from(*a) * f64::from(*b))
            .sum();
        let norm = |vector: &[f32]| {
            vector
                .iter()
                .map(|v| f64::from(*v).powi(2))
                .sum::<f64>()
                .sqrt()
        };
        let denominator = norm(&left) * norm(&right);
        if denominator > 0.0 {
            return (dot / denominator).max(lexical);
        }
    }
    lexical
}

pub(crate) fn query_embedding(query: &str) -> Option<crate::embedder::StampedVector> {
    #[cfg(not(test))]
    {
        search::query_embedding(query)
    }
    #[cfg(test)]
    {
        let _ = query;
        None
    }
}

#[cfg(test)]
mod tests;
