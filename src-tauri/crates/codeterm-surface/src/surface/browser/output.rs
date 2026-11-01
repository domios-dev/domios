use super::BrowserRuntime;
use crate::surface::runtime::SurfaceCallCtx;
use std::path::{Component, Path, PathBuf};

pub(super) fn parse(args: &[String]) -> Result<(Vec<String>, Option<String>), String> {
    let mut rest = Vec::new();
    let mut output = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--out" {
            i += 1;
            if output
                .replace(args.get(i).ok_or("--out needs a file")?.clone())
                .is_some()
            {
                return Err("duplicate --out".into());
            }
        } else {
            rest.push(args[i].clone());
        }
        i += 1;
    }
    Ok((rest, output))
}

fn write(ctx: &SurfaceCallCtx, name: &str, text: &str) -> Result<PathBuf, String> {
    let caller = ctx
        .caller_tab_id
        .as_deref()
        .or(ctx.owner_tab_id.as_deref())
        .ok_or("caller_pad_unavailable")?;
    let pad = ctx.host.agent_pad(caller).ok_or("caller_pad_unavailable")?;
    let pad = std::fs::canonicalize(pad).map_err(|_| "caller_pad_unavailable")?;
    let pad = PathBuf::from(codeterm_paths::strip_verbatim_prefix(
        &pad.to_string_lossy(),
    ));
    let requested = Path::new(name);
    let relative = if requested.is_absolute() {
        requested
            .strip_prefix(&pad)
            .map_err(|_| "output_outside_caller_pad")?
    } else {
        requested
    };
    if relative.as_os_str().is_empty() {
        return Err("invalid_output_path".into());
    }
    for component in relative.components() {
        let Component::Normal(value) = component else {
            return Err("invalid_output_path".into());
        };
        let value = value.to_str().ok_or("invalid_output_path")?;
        let stem = value
            .split('.')
            .next()
            .unwrap_or("")
            .trim_end_matches([' ', '.']);
        let reserved = codeterm_browser::profile::sanitize_profile_name(stem) == format!("_{stem}");
        if value.is_empty()
            || reserved
            || value.contains([':', '\\', '<', '>', '"', '|', '?', '*'])
            || value.ends_with(['.', ' '])
        {
            return Err("invalid_output_path".into());
        }
    }
    let path = pad.join(relative);
    codeterm_util::safe_tree::refuse_linked_ancestors(&path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("output_write_failed: {e}"))?;
    }
    std::fs::write(&path, text).map_err(|e| format!("output_write_failed: {e}"))?;
    Ok(path)
}

impl BrowserRuntime {
    pub(super) async fn output_verb(
        &self,
        ctx: &SurfaceCallCtx,
        output: Option<String>,
        text: String,
    ) -> Result<String, String> {
        let text = self.redact_text(&text);
        let Some(output) = output else {
            return Ok(text);
        };
        let ctx = ctx.clone();
        let path = tokio::task::spawn_blocking(move || write(&ctx, &output, &text))
            .await
            .map_err(|_| "output_task_failed")??;
        Ok(path.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests;
