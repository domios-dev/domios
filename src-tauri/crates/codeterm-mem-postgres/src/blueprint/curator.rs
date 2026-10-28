use super::metadata;
use crate::items;
use codeterm_blueprint_types::blueprint_memory::CuratorEvidence;
use codeterm_mem::{
    backend::WriteContext, solution_metadata::SolutionMetadata, store::MemUpdate, NewMemItem,
};
use postgres::Transaction;
pub(super) fn save(
    tx: &mut Transaction<'_>,
    ctx: &WriteContext,
    mut item: NewMemItem,
    improve: Option<&str>,
    proof: CuratorEvidence,
    now: i64,
) -> Result<String, String> {
    let mut metadata = match improve {
        Some(id) => metadata::read(tx, id)?.unwrap_or_default(),
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
        if items::get_in(tx, id)?.is_none() {
            return Err("covered skill disappeared".into());
        }
        items::update_in(
            tx,
            ctx,
            id,
            &MemUpdate {
                body: Some(item.body),
                tags: Some(item.tags),
                ..Default::default()
            },
        )?;
        id.to_owned()
    } else {
        items::insert_in(tx, ctx, &item, None)?
    };
    metadata::write(tx, &id, &metadata)?;

    Ok(id)
}
