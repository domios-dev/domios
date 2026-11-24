use std::path::PathBuf;

pub mod canvas_watch;
pub(crate) mod model_copies;
pub(super) mod pad_shares;
pub mod reference_inventory;
pub use pad_shares::PadAttachments;

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

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub(super) struct TabAuthority {
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
    paths: &[String],
) -> Result<PreviewPolicy, String> {
    let started = std::time::Instant::now();
    let authority = tab_authority(svc, tab_id)?;
    let mut generated = std::collections::HashSet::new();
    for path in paths {
        if model_copies::lookup(svc, tab_id, path, &authority)? {
            generated.insert(references::path_key(std::path::Path::new(path)));
        }
    }
    let authority_us = started.elapsed().as_micros();
    let transcript_started = std::time::Instant::now();
    let references = if !generated.is_empty() && generated.len() == paths.len() {
        generated.into()
    } else {
        match reference_inventory::lookup(svc, tab_id) {
            Ok(Some(references)) if generated.is_empty() => references,
            Ok(Some(references)) => {
                generated.extend(references.iter().cloned());
                generated.into()
            }
            Ok(None) => {
                let (messages, _) = super::tab_chat::tab_chat_snapshot(svc, tab_id, 0)?;
                generated.extend(references::referenced_paths(
                    &messages,
                    authority.cwd.as_deref(),
                ));
                generated.into()
            }
            Err(_) if !generated.is_empty() => generated.into(),
            Err(error) => return Err(error),
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
    let agent_pads = svc.agent_pad_data_dir.join("runtime").join("agent-pads");
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
            agent_pads: tab_id.map(|_| {
                PathBuf::from(crate::paths::strip_verbatim_prefix(
                    &agent_pads.to_string_lossy(),
                ))
            }),
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
    read_with(
        tab_policy(svc, tab_id, configured, &[path.to_owned()]),
        path,
        kind,
    )
}

pub fn read_owner_share(
    svc: &super::ServiceContext,
    tab_id: &str,
    configured: &[String],
    path: &str,
    kind: &str,
    canvas: Option<&str>,
) -> Result<Option<LocalPreviewFile>, String> {
    pad_shares::read(svc, tab_id, configured, path, kind, canvas)
}

/// Reads an item of a `<ds-canvas src>` file: the tab must reference the canvas, and the item
/// must be one the canvas names.
pub fn read_canvas_item(
    svc: &super::ServiceContext,
    tab_id: &str,
    configured: &[String],
    canvas: &str,
    path: &str,
    kind: &str,
) -> Result<LocalPreviewFile, String> {
    let policy = tab_policy(svc, tab_id, configured, &[canvas.to_owned()]).and_then(|policy| {
        let file = read_local_preview_file_guarded(canvas, "canvas", |path, canonical| {
            policy.authorize(path, canonical)
        })?;
        let items = codeterm_local_preview::artifact_directive::canvas_file_paths(&file.data)
            .ok_or("Canvas file is not a valid canvas")?;
        Ok(policy.with_canvas_items(items))
    });
    read_with(policy, path, kind)
}

fn read_with(
    policy: Result<PreviewPolicy, String>,
    path: &str,
    kind: &str,
) -> Result<LocalPreviewFile, String> {
    let result = policy.and_then(|policy| {
        if kind == "image-original" {
            return html_preview::read_original_image_file(
                path,
                &policy,
                super::image_attachments::MAX_SOURCE_BYTES,
            );
        }
        if kind == "image" {
            let source = std::path::Path::new(path);
            if matches!(
                super::image_attachments::extension(source).as_str(),
                "gif" | "webp" | "svg"
            ) {
                return html_preview::read_local_image_file(path, &policy);
            }
            return super::image_attachments::preview(source, || {
                html_preview::read_image_artifact_file(
                    path,
                    &policy,
                    super::image_attachments::MAX_SOURCE_BYTES,
                )
            });
        }
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
    let policy = tab_policy(svc, tab_id, configured, &paths)?;
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

#[derive(Debug, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CanvasWriteResponse {
    Written,
    Conflict { current: String },
}

/// Owner write-back of a referenced canvas file, refused for upload and mesh-delivered copies.
pub fn write_canvas(
    svc: &super::ServiceContext,
    tab_id: &str,
    configured: &[String],
    path: &str,
    base: &str,
    document: &str,
) -> Result<CanvasWriteResponse, String> {
    let policy = tab_policy(svc, tab_id, configured, &[path.to_owned()])?;
    let outcome = html_preview::write_canvas_file(
        path,
        |path, canonical| policy.authorize_write(path, canonical),
        base,
        document,
    );
    svc.record_product_canvas_file(&outcome, base, document);
    Ok(match outcome? {
        html_preview::CanvasWrite::Written => CanvasWriteResponse::Written,
        html_preview::CanvasWrite::Conflict(current) => CanvasWriteResponse::Conflict { current },
    })
}

#[derive(Debug, serde::Serialize)]
pub struct CanvasWatchResponse {
    pub writable: bool,
    /// `false` when every watch slot is held by an open viewer: the board shows without live updates.
    pub watching: bool,
}

/// Starts change events for a canvas file the tab may read, and says whether the owner may write
/// it; `watch: false` releases this viewer's hold.
pub fn watch_canvas(
    svc: &super::ServiceContext,
    tab_id: &str,
    configured: &[String],
    path: &str,
    watch: bool,
) -> Result<CanvasWatchResponse, String> {
    if !watch {
        canvas_watch::watches().release(tab_id, path);
        return Ok(CanvasWatchResponse {
            writable: false,
            watching: false,
        });
    }
    let policy = tab_policy(svc, tab_id, configured, &[path.to_owned()])?;
    html_preview::probe_canvas_file(path, |path, canonical| policy.authorize(path, canonical))?;
    let writable = html_preview::probe_canvas_file(path, |path, canonical| {
        policy.authorize_write(path, canonical)
    })
    .is_ok();
    let canonical = html_preview::checked_path(path)?
        .canonicalize()
        .map_err(|error| format!("Unable to resolve preview file: {error}"))?;
    let watching = canvas_watch::watches().watch(svc.ui.clone(), tab_id, path, &canonical)?;
    Ok(CanvasWatchResponse { writable, watching })
}
