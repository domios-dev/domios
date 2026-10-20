use std::path::PathBuf;

use crate::service::local_preview::html_preview::{
    check_local_preview_files_guarded, read_local_preview_file_guarded, LocalPreviewFile,
    LocalPreviewFiles,
};

use codeterm_local_preview::PreviewPolicy;
pub use codeterm_local_preview::{html_preview, references};

fn agent_pad_screenshots(svc: &super::ServiceContext, tab_id: &str) -> Option<PathBuf> {
    let pad = super::agent_pad::AgentPad::open(&svc.agent_pad_data_dir, tab_id).ok()?;
    let pad_root = pad.path().canonicalize().ok()?;
    let screenshots = pad.path().join("screenshots").canonicalize().ok()?;
    (screenshots.parent() == Some(pad_root.as_path())).then_some(screenshots)
}

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
    let data_dir = crate::paths::data_dir();
    denied.push(data_dir.clone());
    let mut evidence_denied = configured.clone();
    if let Some(home) = dirs::home_dir() {
        let ssh = home.join(".ssh");
        denied.push(ssh.clone());
        evidence_denied.push(ssh);
    }
    let mut policy = PreviewPolicy::new(references, &denied)?
        .with_uploads(&crate::service::web_access::uploads_dir(), &configured)?;
    for bucket in ["chat", "images", "files"] {
        policy = policy.with_uploads(&crate::attachments::bucket_dir(bucket), &configured)?;
    }
    if let Some(screenshots) = agent_pad_screenshots(svc, tab_id) {
        policy = policy.with_referenced_uploads(&screenshots, &evidence_denied)?;
    }
    Ok(policy)
}

pub fn read(
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

pub fn check(
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
