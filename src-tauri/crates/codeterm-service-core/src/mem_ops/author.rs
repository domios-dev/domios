use crate::mem::store::MemAuthor;
use crate::service::ServiceContext;

#[derive(Default)]
pub(crate) struct OwnerMemoryAuthor(pub parking_lot::RwLock<Option<String>>);

pub async fn mem_author_for_rpc(
    svc: &ServiceContext,
    tab_id: Option<&str>,
    method: &str,
) -> Result<MemAuthor, String> {
    if method != "mem.neighbor_save" {
        let authority = tab_id.and_then(|id| {
            svc.mux
                .read()
                .get_tab(id)
                .and_then(|tab| tab.neighbor_authority.clone())
        });
        if let Some(authority) = authority {
            return Ok(MemAuthor {
                kind: crate::mem::store::MemAuthorKind::Neighbor,
                account_uid: None,
                friendship_id: Some(authority.friendship_id),
                label: None,
            });
        }
    }
    mem_author_for_caller(svc, tab_id).await
}

pub async fn mem_author_for_caller(
    svc: &ServiceContext,
    tab_id: Option<&str>,
) -> Result<MemAuthor, String> {
    if let Some(tab_id) = tab_id {
        let (neighbor, worker_id) = {
            let mux = svc.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return Err("neighbor_memory_author_unavailable: Caller is closed".into());
            };
            (tab.neighbor_authority.clone(), tab.worker_id.clone())
        };
        if let Some(authority) = neighbor {
            let peer_uid = svc.neighbor_memory_peer_uid(&authority).await?;
            return Ok(MemAuthor::neighbor(peer_uid, authority.friendship_id));
        }
        if let Some(worker_id) = worker_id {
            return Ok(MemAuthor::system(
                svc.workers
                    .get(&worker_id)
                    .map_or_else(|| "worker".into(), |worker| worker.intent),
            ));
        }
    }
    Ok(MemAuthor::owner(
        svc.ext::<OwnerMemoryAuthor>().0.read().clone(),
    ))
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub struct FixtureNeighborAuthors(
    pub parking_lot::RwLock<std::collections::HashMap<(String, String), String>>,
);

pub fn author_audit_label(author: &MemAuthor) -> String {
    match author.kind {
        crate::mem::store::MemAuthorKind::Owner => author
            .account_uid
            .as_ref()
            .map_or_else(|| "owner".into(), |uid| format!("owner:{uid}")),
        crate::mem::store::MemAuthorKind::Neighbor => format!(
            "neighbor:{}",
            author.account_uid.as_deref().unwrap_or_default()
        ),
        crate::mem::store::MemAuthorKind::System => {
            format!("system:{}", author.label.as_deref().unwrap_or("unscoped"))
        }
    }
}

#[cfg(test)]
mod tests;
