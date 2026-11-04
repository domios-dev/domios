use std::path::PathBuf;

pub mod reference_inventory;

use crate::service::local_preview::html_preview::{
    check_local_preview_files_guarded, read_local_preview_file_guarded, LocalPreviewFile,
    LocalPreviewFiles,
};

use codeterm_local_preview::PreviewPolicy;
pub use codeterm_local_preview::{html_preview, references};

fn agent_pad_root(svc: &super::ServiceContext, tab_id: &str) -> Option<PathBuf> {
    svc.mux.read().get_tab(tab_id)?;
    let pad = super::agent_pad::AgentPad::open(&svc.agent_pad_data_dir, tab_id).ok()?;
    let root = pad.path().canonicalize().ok()?;
    let pads = pad.path().parent()?.canonicalize().ok()?;
    (root.parent() == Some(pads.as_path()) && root.file_name()?.to_str()? == tab_id).then_some(root)
}

#[derive(Clone, PartialEq)]
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
    let started = std::time::Instant::now();
    let authority = tab_authority(svc, tab_id)?;
    let authority_us = started.elapsed().as_micros();
    let transcript_started = std::time::Instant::now();
    let references = match reference_inventory::lookup(svc, tab_id)? {
        Some(references) => references,
        None => {
            let (messages, _) = super::tab_chat::tab_chat_snapshot(svc, tab_id, 0)?;
            references::referenced_paths(&messages, authority.cwd.as_deref()).into()
        }
    };
    let transcript_us = transcript_started.elapsed().as_micros();
    let binding_started = std::time::Instant::now();
    if authority != tab_authority(svc, tab_id)? {
        return Err("Preview session changed".into());
    }
    let binding_us = binding_started.elapsed().as_micros();
    let policy_started = std::time::Instant::now();
    let count = references.len();
    let policy = artifact_policy(svc, Some(tab_id), Default::default(), configured)?
        .with_reference_inventory(references);
    log::debug!(
        "[preview.policy] tab={} authority_us={} references_us={} binding_us={} roots_us={} total_us={} references={}",
        tab_id, authority_us, transcript_us, binding_us,
        policy_started.elapsed().as_micros(), started.elapsed().as_micros(), count
    );
    Ok(policy)
}

pub(super) fn artifact_policy(
    svc: &super::ServiceContext,
    tab_id: Option<&str>,
    references: std::collections::HashSet<String>,
    configured: &[String],
) -> Result<PreviewPolicy, String> {
    codeterm_local_preview::PreviewPolicy::for_context(
        references,
        codeterm_local_preview::PreviewRoots {
            configured: configured.iter().map(PathBuf::from).collect(),
            private: [crate::paths::data_dir(), svc.agent_pad_data_dir.clone()]
                .iter()
                .map(|path| {
                    PathBuf::from(crate::paths::strip_verbatim_prefix(&path.to_string_lossy()))
                })
                .collect(),
            uploads: std::iter::once(crate::service::web_access::uploads_dir())
                .chain(["chat", "images", "files"].map(crate::attachments::bucket_dir))
                .collect(),
            own_pad: tab_id.and_then(|tab| agent_pad_root(svc, tab)),
            workspace: tab_id
                .and_then(|tab| svc.mux.read().get_tab(tab).and_then(|tab| tab.cwd.clone()))
                .map(PathBuf::from),
            ssh: dirs::home_dir().map(|home| home.join(".ssh")),
        },
    )
}

pub fn read(
    svc: &super::ServiceContext,
    tab_id: &str,
    configured: &[String],
    path: &str,
    kind: &str,
) -> Result<LocalPreviewFile, String> {
    let result = tab_policy(svc, tab_id, configured).and_then(|policy| {
        read_local_preview_file_guarded(path, kind, |path, canonical| {
            policy.authorize(path, canonical)
        })
    });
    if let Err(error) = &result {
        log::debug!(
            "[local-preview] file refused: {}",
            html_preview::refusal_reason(error)
        );
    }
    result
}

pub fn check(
    svc: &super::ServiceContext,
    tab_id: &str,
    configured: &[String],
    paths: Vec<String>,
) -> Result<LocalPreviewFiles, String> {
    if paths.is_empty() {
        tab_authority(svc, tab_id)?;
        return Ok(check_local_preview_files_guarded(paths, |_, _| {
            Err("No preview paths".into())
        }));
    }
    let policy = tab_policy(svc, tab_id, configured)?;
    let count = paths.len();
    let started = std::time::Instant::now();
    let result = check_local_preview_files_guarded(paths, |path, canonical| {
        policy.authorize(path, canonical)
    });
    log::debug!(
        "[preview.files] tab={} count={} elapsed_us={}",
        tab_id,
        count,
        started.elapsed().as_micros()
    );
    Ok(result)
}
