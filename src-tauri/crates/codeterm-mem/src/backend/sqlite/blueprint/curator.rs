use super::*;
use crate::solution_metadata::SolutionMetadata;
use crate::NewMemItem;
use codeterm_blueprint_types::blueprint_memory::CuratorEvidence;
impl SqliteMemStore {
    pub(super) fn save_curator_skill(
        &self,
        ctx: &WriteContext,
        mut item: NewMemItem,
        improve: Option<&str>,
        proof: CuratorEvidence,
        now: i64,
    ) -> Result<String, String> {
        {
            let conn = self.conn.lock();
            let tx = conn
                .unchecked_transaction()
                .map_err(|error| error.to_string())?;
            let mut metadata = match improve {
                Some(id) => metadata::read(&tx, id)?.unwrap_or_default(),
                None => SolutionMetadata::default(),
            };
            metadata.content_fingerprint =
                Some(codeterm_blueprint_types::blueprint_memory::curator::fingerprint(&item.body));
            metadata.observe_curator(&proof, now);
            item.body.push_str(&format!(
                "\n\nCurator evidence: {}{}",
                proof.episodes.join(" "),
                if proof.granted {
                    format!(" grant:{}:{}", proof.plan, proof.round)
                } else {
                    String::new()
                }
            ));
            item.tags
                .retain(|tag| !tag.starts_with("curator:candidate:"));
            item.tags.extend(["curator".into(), proof.plan]);
            let id = if let Some(id) = improve {
                if store::get(&tx, id, &self.blobs_dir)?.is_none() {
                    return Err("covered skill disappeared".into());
                }
                store::update_with_context(
                    ctx,
                    &tx,
                    id,
                    &store::MemUpdate {
                        body: Some(item.body),
                        tags: Some(item.tags),
                        ..Default::default()
                    },
                    &self.blobs_dir,
                )?;
                id.to_owned()
            } else {
                store::insert_with_context(ctx, &tx, &item, &self.blobs_dir)?
            };
            metadata::write(&tx, &id, &metadata)?;
            tx.commit().map_err(|error| error.to_string())?;
            Ok(id)
        }
    }
}
