use super::*;
use crate::{search, SearchHit, SearchQuery};
use std::collections::BTreeSet;
impl SqliteMemStore {
    pub(super) fn solution_candidates(
        &self,
        query: &SearchQuery,
        source: Option<&str>,
        embedding: Option<&crate::embedder::StampedVector>,
    ) -> Result<(Vec<SearchHit>, BTreeSet<String>), String> {
        let cfg = search::SearchConfig {
            source_filter: Some(source.map(str::to_owned)),
            ..Default::default()
        };
        let conn = self.conn.lock();
        let hits =
            search::hybrid_rrf_with_embedding(&conn, query, &self.blobs_dir, &cfg, embedding)?;
        let relevant_vectors = embedding
            .and_then(|vector| search::scoped_vectors(&conn, vector, 250, query, &cfg).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, distance)| {
                distance.is_finite() && 1.0 - distance >= crate::solution_search::RELEVANCE_FLOOR
            })
            .map(|(id, _)| id)
            .collect::<BTreeSet<_>>();
        Ok((hits, relevant_vectors))
    }
}
