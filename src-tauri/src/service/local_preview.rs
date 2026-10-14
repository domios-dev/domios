use std::path::PathBuf;

use crate::commands::html_preview::{
    check_local_preview_files_guarded, read_local_preview_file_guarded, LocalPreviewFile,
    LocalPreviewFiles,
};

mod policy;
mod references;
use policy::PreviewPolicy;

#[derive(PartialEq)]
struct TabAuthority {
    backing: crate::multiplexer::types::TabBacking,
    provider: Option<String>,
    session: Option<String>,
    nonce: Option<String>,
    cwd: Option<String>,
}

fn tab_authority(svc: &super::ServiceContext, tab_id: &str) -> Result<TabAuthority, String> {
    let mux = svc.mux.read();
    let tab = mux.get_tab(tab_id).ok_or("Tab not found")?;
    Ok(TabAuthority {
        backing: tab.backing.clone(),
        provider: tab.agent_type.clone(),
        session: tab.agent_session_id.clone(),
        nonce: tab.session_binding_nonce.clone(),
        cwd: tab.cwd.clone(),
    })
}

fn tab_policy(
    svc: &super::ServiceContext,
    tab_id: &str,
    configured: &[String],
) -> Result<PreviewPolicy, String> {
    let authority = tab_authority(svc, tab_id)?;
    let (messages, _) = super::tab_chat::tab_chat_snapshot(svc, tab_id, 0)?;
    if authority != tab_authority(svc, tab_id)? {
        return Err("Preview session changed".into());
    }
    let references = references::referenced_paths(&messages, authority.cwd.as_deref());
    let configured: Vec<_> = configured.iter().map(PathBuf::from).collect();
    let mut denied = configured.clone();
    denied.push(crate::paths::data_dir());
    if let Some(home) = dirs::home_dir() {
        denied.push(home.join(".ssh"));
    }
    let mut policy = PreviewPolicy::new(references, &denied)?
        .with_uploads(&crate::api::web_routes::uploads_dir(), &configured)?;
    for bucket in ["chat", "images", "files"] {
        policy = policy.with_uploads(&crate::attachments::bucket_dir(bucket), &configured)?;
    }
    Ok(policy)
}

pub(crate) fn read(
    svc: &super::ServiceContext,
    tab_id: &str,
    configured: &[String],
    path: &str,
    kind: &str,
) -> Result<LocalPreviewFile, String> {
    let policy = tab_policy(svc, tab_id, configured)?;
    read_local_preview_file_guarded(path, kind, |path, canonical| {
        policy.authorize(path, canonical)
    })
}

pub(crate) fn check(
    svc: &super::ServiceContext,
    tab_id: &str,
    configured: &[String],
    paths: Vec<String>,
) -> Result<LocalPreviewFiles, String> {
    let policy = tab_policy(svc, tab_id, configured)?;
    Ok(check_local_preview_files_guarded(
        paths,
        |path, canonical| policy.authorize(path, canonical),
    ))
}

#[cfg(test)]
mod tests;
