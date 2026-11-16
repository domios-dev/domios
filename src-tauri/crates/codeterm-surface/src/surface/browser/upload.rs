use std::fs::File;
use std::path::PathBuf;
#[cfg(any(windows, test))]
use std::path::{Component, Path};

use serde_json::{json, Value};

use super::{BrowserRuntime, SurfaceNotice, WAIT_DEADLINE};
use crate::surface::runtime::SurfaceCallCtx;
use codeterm_browser::PageChannel;

const MAX_FILES: usize = 64;

pub(super) struct AdmittedFile {
    path: PathBuf,
    name: String,
    size: u64,
    grant: Option<PathBuf>,
    _pins: Vec<File>,
}

#[cfg(any(windows, test))]
fn relative_path<'a>(pad: &Path, name: &'a str) -> Result<&'a Path, String> {
    if name.len() > 4096 {
        return Err("invalid_upload_path".into());
    }
    let requested = Path::new(name);
    let relative = if requested.is_absolute() {
        requested
            .strip_prefix(pad)
            .map_err(|_| "upload_outside_caller_pad")?
    } else {
        requested
    };
    if relative.as_os_str().is_empty() {
        return Err("invalid_upload_path".into());
    }
    if relative.components().count() > 32 {
        return Err("invalid_upload_path".into());
    }
    for component in relative.components() {
        let Component::Normal(value) = component else {
            return Err("invalid_upload_path".into());
        };
        let value = value.to_str().ok_or("invalid_upload_path")?;
        let stem = value
            .split('.')
            .next()
            .unwrap_or("")
            .trim_end_matches([' ', '.']);
        let device_stem: String = stem
            .chars()
            .map(|ch| match ch {
                '\u{00b9}' => '1',
                '\u{00b2}' => '2',
                '\u{00b3}' => '3',
                other => other,
            })
            .collect();
        if value.is_empty()
            || codeterm_browser::profile::sanitize_profile_name(&device_stem)
                == format!("_{device_stem}")
            || ["CONIN$", "CONOUT$", "CLOCK$"]
                .iter()
                .any(|name| stem.eq_ignore_ascii_case(name))
            || value.contains([':', '\\', '<', '>', '"', '|', '?', '*'])
            || value.chars().any(char::is_control)
            || value.ends_with(['.', ' '])
        {
            return Err("invalid_upload_path".into());
        }
    }
    Ok(relative)
}

#[cfg(any(windows, test))]
fn admission_path(
    pad: &Path,
    grants: &[PathBuf],
    name: &str,
) -> Result<(PathBuf, Option<PathBuf>), String> {
    let requested = Path::new(name);
    if name
        .split(['/', '\\'])
        .any(|part| matches!(part, "." | ".."))
    {
        return Err("invalid_upload_path".into());
    }
    if !requested.is_absolute() || requested.starts_with(pad) {
        return relative_path(pad, name).map(|relative| (pad.join(relative), None));
    }
    let root = grants
        .iter()
        .filter(|root| root.is_absolute() && requested.starts_with(root))
        .max_by_key(|root| root.components().count())
        .ok_or("upload_outside_caller_pad_or_granted_folder")?;
    for part in root.components() {
        match part {
            Component::Normal(value) => {
                relative_path(Path::new(""), value.to_str().ok_or("invalid_upload_path")?)?;
            }
            Component::RootDir => {}
            #[cfg(windows)]
            Component::Prefix(prefix) if matches!(prefix.kind(), std::path::Prefix::Disk(_)) => {}
            _ => return Err("invalid_upload_path".into()),
        }
    }
    if root
        .to_string_lossy()
        .split(['/', '\\'])
        .any(|part| matches!(part, "." | ".."))
    {
        return Err("invalid_upload_path".into());
    }
    let relative = relative_path(root, name)?;
    Ok((root.join(relative), Some(root.clone())))
}

#[cfg(windows)]
fn pin(path: &Path, directory: bool) -> Result<File, String> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 1;
    const FILE_SHARE_WRITE: u32 = 2;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(if directory {
            FILE_SHARE_READ | FILE_SHARE_WRITE
        } else {
            FILE_SHARE_READ
        })
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|e| format!("upload_open_failed: {e}"))?;
    let metadata = file.metadata().map_err(|e| e.to_string())?;
    if codeterm_util::safe_tree::is_link(&metadata)
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err("upload_requires_regular_file_without_links".into());
    }
    Ok(file)
}

#[cfg(windows)]
fn admit(
    ctx: &SurfaceCallCtx,
    names: &[String],
    grants: &[PathBuf],
) -> Result<Vec<AdmittedFile>, String> {
    let caller = ctx
        .caller_tab_id
        .as_deref()
        .ok_or("caller_pad_unavailable")?;
    let pad = ctx.host.agent_pad(caller).ok_or("caller_pad_unavailable")?;
    let mut root_pins = Vec::new();
    let mut current = PathBuf::new();
    for component in pad.components() {
        current.push(component);
        if matches!(component, Component::Prefix(_)) {
            continue;
        }
        root_pins.push(pin(&current, true)?);
    }
    let pad = std::fs::canonicalize(pad).map_err(|_| "caller_pad_unavailable")?;
    let pad = PathBuf::from(codeterm_paths::strip_verbatim_prefix(
        &pad.to_string_lossy(),
    ));
    names
        .iter()
        .map(|name| {
            let (path, grant) = admission_path(&pad, grants, name)?;
            let mut pins = Vec::new();
            let mut current = PathBuf::new();
            for component in path.components() {
                current.push(component);
                if matches!(component, Component::Prefix(_)) {
                    continue;
                }
                pins.push(pin(&current, current != path)?);
            }
            let size = pins
                .last()
                .ok_or("invalid_upload_path")?
                .metadata()
                .map_err(|e| e.to_string())?
                .len();
            Ok(AdmittedFile {
                name: path
                    .file_name()
                    .ok_or("invalid_upload_path")?
                    .to_string_lossy()
                    .into_owned(),
                path,
                size,
                grant,
                _pins: pins,
            })
        })
        .collect()
}

#[cfg(not(windows))]
fn admit(_: &SurfaceCallCtx, _: &[String], _: &[PathBuf]) -> Result<Vec<AdmittedFile>, String> {
    Err(format!(
        "unsupported: secure browser upload admission is unavailable on {}",
        std::env::consts::OS
    ))
}

impl BrowserRuntime {
    pub(super) async fn upload_verb(
        &self,
        ctx: &SurfaceCallCtx,
        page: &PageChannel,
        args: &[String],
    ) -> Result<String, String> {
        if args.len() < 2 || args.len() > MAX_FILES + 1 {
            return Err(
                "usage: upload <ref|button text> <pad-relative or granted absolute path>... (maximum 64 files)".into(),
            );
        }
        let _gate = self.upload_gate.try_lock().map_err(|_| "upload_busy")?;
        if self.uploads.lock().len() + args.len() - 1 > MAX_FILES {
            return Err("upload_limit: reopen the browser before uploading more files".into());
        }
        let ctx_copy = ctx.clone();
        let names = args[1..].to_vec();
        let grants = self
            .host
            .named_profile_for_page(page)
            .map(|profile| ctx.host.browser_upload_folders(&profile))
            .unwrap_or_default();
        let files = tokio::task::spawn_blocking(move || admit(&ctx_copy, &names, &grants))
            .await
            .map_err(|_| "upload_admission_failed")??;
        let locator = if args[0].starts_with('@') {
            vec![args[0].clone()]
        } else {
            vec!["button".into(), args[0].clone()]
        };
        let target = self.resolve(&locator)?;
        let paths: Vec<String> = files
            .iter()
            .map(|f| f.path.to_string_lossy().into_owned())
            .collect();
        let summary = files
            .iter()
            .map(|f| match &f.grant {
                Some(folder) => format!(
                    "{} ({} bytes, granted folder {})",
                    f.name,
                    f.size,
                    folder.display()
                ),
                None => format!("{} ({} bytes)", f.name, f.size),
            })
            .collect::<Vec<_>>()
            .join(", ");
        let expected: Vec<Value> = files
            .iter()
            .map(|file| json!({"name":file.name,"size":file.size}))
            .collect();
        self.uploads.lock().extend(files);
        let result = self
            .upload_files(
                page,
                target.backend_node_id,
                &paths,
                &expected,
                &summary,
                ctx.actor.as_deref(),
            )
            .await;
        self.shared.invalidate_snapshot();
        result
    }

    async fn upload_files(
        &self,
        page: &PageChannel,
        target: i64,
        paths: &[String],
        expected: &[Value],
        summary: &str,
        actor: Option<&str>,
    ) -> Result<String, String> {
        self.send(
            page,
            "Page.setInterceptFileChooserDialog",
            json!({"enabled":true}),
        )
        .await
        .map_err(|e| format!("unsupported: file chooser interception unavailable: {e}"))?;
        let mut chooser =
            page.subscribe_session(&["Page.fileChooserOpened", "Page.frameNavigated"]);
        let node = self
            .send(page, "DOM.describeNode", json!({"backendNodeId":target}))
            .await?;
        let direct = node.pointer("/node/nodeName").and_then(Value::as_str) == Some("INPUT")
            && node
                .pointer("/node/attributes")
                .and_then(Value::as_array)
                .is_some_and(|attrs| {
                    attrs.as_chunks::<2>().0.iter().any(|pair| {
                        pair[0] == "type"
                            && pair[1]
                                .as_str()
                                .is_some_and(|s| s.eq_ignore_ascii_case("file"))
                    })
                });
        let id = if direct {
            target
        } else {
            self.click_node(page, target).await.map_err(|e| {
                format!("action_unconfirmed: {e}; inspect the page before another upload")
            })?;
            let event = tokio::time::timeout(WAIT_DEADLINE, chooser.recv()).await.map_err(|_| "upload_unconfirmed: no intercepted chooser; inspect the page, never replay automatically")?.ok_or("upload_unconfirmed: page closed")?;
            if event.method != "Page.fileChooserOpened" {
                return Err("stale: page navigated during upload".into());
            }
            if event.params.get("mode").and_then(Value::as_str) != Some("selectMultiple")
                && paths.len() > 1
            {
                return Err("upload_single_file_only".into());
            }
            event
                .params
                .get("backendNodeId")
                .and_then(Value::as_i64)
                .ok_or("unsupported: chooser has no file input node")?
        };
        let resolved = self
            .send(page, "DOM.resolveNode", json!({"backendNodeId":id}))
            .await?;
        let object = resolved
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or("stale: upload input is unavailable")?;
        let info = self.send(page, "Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function(){return {file:this instanceof HTMLInputElement && this.type==='file' && !this.disabled && !this.webkitdirectory,multiple:this.multiple,origin:this.ownerDocument.location.origin};}","returnByValue":true})).await?;
        let info = info
            .pointer("/result/value")
            .ok_or("stale: upload input is unavailable")?;
        if info.get("file").and_then(Value::as_bool) != Some(true) {
            return Err("upload_requires_enabled_file_input".into());
        }
        if paths.len() > 1 && info.get("multiple").and_then(Value::as_bool) != Some(true) {
            return Err("upload_single_file_only".into());
        }
        let origin = info
            .get("origin")
            .and_then(Value::as_str)
            .filter(|s| s.starts_with("https://") || s.starts_with("http://"))
            .ok_or("upload_requires_http_origin")?;
        let activity = format!("uploading {summary} to {origin}");
        self.shared.set_activity(&activity, actor);
        self.shared.notice(SurfaceNotice::info(&activity));
        self.send(
            page,
            "DOM.setFileInputFiles",
            json!({"files":paths,"backendNodeId":id}),
        )
        .await
        .map_err(|e| {
            format!("upload_unconfirmed: {e}; inspect the page, never replay automatically")
        })?;
        let selected = self.send(page, "Runtime.callFunctionOn", json!({"objectId":object,"functionDeclaration":"function(){return Array.from(this.files || [], f => ({name:f.name,size:f.size}));}","returnByValue":true})).await.map_err(|e| format!("upload_unconfirmed: {e}"))?;
        if selected.pointer("/result/value") != Some(&json!(expected)) {
            return Err("upload_unconfirmed: the input did not retain the selected files; inspect the page before another upload".into());
        }
        Ok(format!(
            "selected {summary} for {origin}; inspect the page to confirm upload completion"
        ))
    }
}

#[cfg(test)]
mod tests;
