use crate::executor::Transaction;
use crate::queries;
use codeterm_mem::{
    backend::blueprint::SolutionCandidates, embedder::StampedVector, search::SearchConfig,
    SearchQuery,
};

pub(super) fn candidates(
    tx: &mut Transaction<'_>,
    query: &SearchQuery,
    source: Option<&str>,
    embedding: Option<&StampedVector>,
) -> Result<SolutionCandidates, String> {
    let cfg = SearchConfig {
        source_filter: Some(source.map(str::to_owned)),
        ..Default::default()
    };
    let hits = queries::recall_with_embedding_in(tx, query, &cfg, embedding)?;
    let relevant = embedding
        .and_then(|vector| queries::scoped_vectors_in(tx, vector, 250, query, &cfg).ok())
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, distance)| distance.is_finite() && 1.0 - distance >= 0.55)
        .map(|(id, _)| id)
        .collect();
    Ok((hits, relevant))
}
