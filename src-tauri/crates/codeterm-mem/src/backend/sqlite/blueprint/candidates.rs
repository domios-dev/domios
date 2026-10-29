use super::*;
use crate::blueprint_library::recall::Candidate;
use crate::embedder::EmbedderIdentity;
impl SqliteMemStore {
    pub(super) fn candidates(
        &self,
        ctx: &WriteContext,
        area: &str,
        team: Option<&str>,
        identity: Option<&EmbedderIdentity>,
    ) -> Result<Vec<Candidate>, String> {
        let conn = self.conn.lock();
        let library = library::load_entries_scoped(ctx, &conn, &self.blobs_dir, area, true, team)?;
        library
            .into_iter()
            .map(|entry| {
                Ok(Candidate {
                    row: index::read_row(&conn, &entry.id)?,
                    vectors: identity
                        .map(|identity| index::read_vectors(&conn, &entry.id, identity))
                        .transpose()?
                        .unwrap_or_default(),
                    entry,
                })
            })
            .collect()
    }
}
