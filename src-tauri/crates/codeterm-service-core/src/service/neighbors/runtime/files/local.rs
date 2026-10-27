use super::*;
use crate::service::tab_credentials::TabCaller;

#[cfg(test)]
mod tests;

async fn concierge_file_views(metadata: &Metadata, friendship: &str, entries: &[storage::Entry]) -> Value {
    let mut files = vec![];
    for entry in entries.iter().filter(|entry| entry.direction == "in" && entry.state == "saved") {
        if let Ok(path) = metadata.saved_file(friendship, entry).await {
            let mut view = json!(entry);
            view["path"] = json!(path.to_string_lossy());
            files.push(view);
        }
    }
    json!({"files":files})
}

impl ServiceContext {
    pub(super) async fn neighbor_file_local(
        &self,
        command: &Command,
        concierge: Option<&TabCaller>,
    ) -> Result<Value, String> {
        let generation = self.neighbor_history_generation();
        let account = self
            .ext::<crate::service::account::AccountCell>()
            .0
            .get()
            .cloned()
            .ok_or("Sign in to use Neighbor files")?;
        let session = account
            .session()
            .await
            .ok_or("Sign in to use Neighbor files")?;
        let metadata = Metadata::new(self.account_data_root()?, &session.identity.uid);
        let friendship = command.friendship();
        let value: codeterm_neighbors::Result<Value> = async {
            match command {
                Command::List { .. } => {
                    let entries = metadata.file_entries(friendship).await?;
                    if concierge.is_some() {
                        return Ok(concierge_file_views(&metadata, friendship, &entries).await);
                    }
                    Ok(json!({"folder":metadata.friend_folder(friendship)?.to_string_lossy(),"files":entries}))
                }
                Command::Open { file_id, .. } => {
                    let entries = metadata.file_entries(friendship).await?;
                    let entry = entries.iter().find(|entry| &entry.file_id == file_id && entry.direction == "in" && entry.state == "saved").ok_or(Error::InvalidRequest)?;
                    let path = metadata.saved_file(friendship, entry).await?;
                    if concierge.is_some() {
                        let mut view = json!(entry);
                        view["path"] = json!(path.to_string_lossy());
                        return Ok(view);
                    }
                    storage::open(path).await?;
                    Ok(json!({"opened":true}))
                }
                Command::OpenFolder { .. } if concierge.is_none() => {
                    storage::open_folder(metadata.friend_folder(friendship)?).await?;
                    Ok(json!({"opened":true}))
                }
                _ => Err(Error::PermissionDenied),
            }
        }.await;
        self.require_neighbor_generation(generation)?;
        if !crate::service::neighbors::same_session(account.session().await.as_ref(), &session) {
            return Err("neighbor_forbidden: Neighbor account changed".into());
        }
        if let Some(caller) = concierge {
            self.authorize_neighbor_file_read(caller, friendship)
                .await?;
        }
        value.map_err(|error| error.to_string())
    }
}
