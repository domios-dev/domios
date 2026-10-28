use crate::mem::store::MemAuthor;
use crate::service::ServiceContext;

#[derive(Default)]
pub(crate) struct OwnerMemoryAuthor(pub parking_lot::RwLock<Option<String>>);

pub fn mem_author_for_caller(svc: &ServiceContext, tab_id: Option<&str>) -> MemAuthor {
    if let Some(tab_id) = tab_id {
        let (neighbor, worker_id) = {
            let mux = svc.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return MemAuthor::system("closed-caller");
            };
            (tab.neighbor_authority.clone(), tab.worker_id.clone())
        };
        if let Some(authority) = neighbor {
            return MemAuthor::neighbor(authority.uid, authority.friendship_id);
        }
        if let Some(worker_id) = worker_id {
            return MemAuthor::system(
                svc.workers
                    .get(&worker_id)
                    .map_or_else(|| "worker".into(), |worker| worker.intent),
            );
        }
    }
    MemAuthor::owner(svc.ext::<OwnerMemoryAuthor>().0.read().clone())
}

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
