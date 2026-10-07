use crate::agent::provider::{AgentLaunchParams, AgentProvider, ContextAttach};
use crate::plan::TaskFiles;
use crate::service::{ServiceContext, SpawnAgentParams};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};

pub(crate) fn validate_support(
    files: &TaskFiles,
    has_skills: bool,
    capability: Option<ContextAttach>,
) -> Result<(), String> {
    let capability = capability.unwrap_or_default();
    if !files.edit.is_empty() && !capability.editable {
        return Err("provider cannot attach editable task files".into());
    }
    if (!files.read.is_empty() || has_skills) && !capability.read_only {
        return Err("provider cannot attach read-only task context".into());
    }
    Ok(())
}

fn resolve_path(cwd: &Path, path: &str) -> Result<PathBuf, String> {
    let candidate = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        cwd.join(path)
    };
    if !candidate.is_absolute() {
        return Err("Worker cwd must be absolute for task context".into());
    }
    let mut normalized = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err("task context escapes filesystem root".into());
                }
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    let mut ancestor = normalized.as_path();
    let mut missing = Vec::new();
    while !ancestor.exists() {
        missing.push(
            ancestor
                .file_name()
                .ok_or("task context has no existing ancestor")?
                .to_os_string(),
        );
        ancestor = ancestor
            .parent()
            .ok_or("task context has no existing ancestor")?;
    }
    let mut canonical = ancestor
        .canonicalize()
        .map_err(|e| format!("cannot resolve task context: {e}"))?;
    for name in missing.into_iter().rev() {
        canonical.push(name);
    }
    Ok(PathBuf::from(codeterm_paths::strip_verbatim_prefix(
        &canonical.to_string_lossy(),
    )))
}

pub(crate) fn resolve_files(cwd: &Path, files: TaskFiles) -> Result<TaskFiles, String> {
    let files = files.validated()?;
    let resolve = |paths: Vec<String>, read_only: bool| -> Result<Vec<String>, String> {
        let mut result = Vec::new();
        for path in paths {
            let path = resolve_path(cwd, &path)?;
            if read_only {
                if !path.is_file() {
                    return Err(format!(
                        "read-only task file does not exist: {}",
                        path.display()
                    ));
                }
                std::fs::File::open(&path)
                    .map_err(|e| format!("cannot read task file {}: {e}", path.display()))?;
            } else if path.exists() && !path.is_file() {
                return Err(format!(
                    "editable task path is not a file: {}",
                    path.display()
                ));
            }
            let text = path.to_string_lossy().into_owned();
            if !result
                .iter()
                .any(|other: &String| codeterm_paths::paths_equal(other, &text))
            {
                result.push(text);
            }
        }
        Ok(result)
    };
    let edit = resolve(files.edit, false)?;
    let read = resolve(files.read, true)?;
    if edit
        .iter()
        .any(|a| read.iter().any(|b| codeterm_paths::paths_equal(a, b)))
    {
        return Err("resolved task context cannot be both editable and read-only".into());
    }
    Ok(TaskFiles { edit, read })
}

pub(crate) fn prepare(
    service: &ServiceContext,
    provider_id: &str,
    params: &mut SpawnAgentParams,
) -> Result<(), String> {
    let (tool_less, capability) = service
        .agent_registry
        .with_provider(provider_id, |provider| {
            (provider.is_tool_less(), provider.context_attach())
        })
        .unwrap_or_default();
    if !tool_less {
        return Ok(());
    }
    let Some(authority) = params.task_launch_authority.as_ref() else {
        return Ok(());
    };
    let detail = service
        .plan_store
        .show_task(&authority.task_id)
        .ok_or("task context task not found")?;
    if detail.plan_id != authority.plan_id {
        return Err("task context plan does not match attempt".into());
    }
    let manifest = params
        .plan_skill_manifest
        .as_deref()
        .ok_or("task context requires the captured attempt skill manifest")?;
    validate_support(&detail.task.files, !manifest.is_empty(), capability)?;
    let files = resolve_files(
        Path::new(
            params
                .cwd
                .as_deref()
                .ok_or("task context requires a Worker cwd")?,
        ),
        detail.task.files,
    )?;
    let mut context = String::new();
    for entry in manifest {
        let skill = crate::service::plan::plan_skills::show_plan_skill(
            &service.plan_store,
            &authority.plan_id,
            &entry.slug,
        )
        .map_err(|e| e.to_string())?;
        append_skill(
            &mut context,
            &entry.slug,
            &entry.content_digest,
            &skill.body,
        )?;
    }
    params.context_files = Some(files);
    params.context_skills = (!context.is_empty()).then_some(context);
    Ok(())
}

fn append_skill(bundle: &mut String, slug: &str, digest: &str, body: &str) -> Result<(), String> {
    if format!("sha256:{:x}", Sha256::digest(body.as_bytes())) != digest {
        return Err(format!(
            "plan skill changed after attempt manifest capture: {slug}"
        ));
    }
    if bundle.len() + body.len() + slug.len() + 32 > 256 * 1024 {
        return Err("plan skill context exceeds 256 KiB".into());
    }
    bundle.push_str(&format!("\n\n# Plan skill: {slug}\n\n{body}"));
    Ok(())
}

pub(crate) fn build(
    provider: &dyn AgentProvider,
    params: &AgentLaunchParams,
    skills: Option<&str>,
) -> String {
    let Some(params) = attach_skills(params, skills, |body| {
        codeterm_util::task_files::write(body, Some(".plan-skills.md"))
    }) else {
        log::error!("[launch] mandatory plan skill context could not be materialized");
        return String::new();
    };
    crate::service::tool_less_instructions::build(provider, &params)
}

fn attach_skills(
    params: &AgentLaunchParams,
    skills: Option<&str>,
    materialize: impl FnOnce(&str) -> Option<String>,
) -> Option<AgentLaunchParams> {
    let mut params = params.clone();
    if let Some(skills) = skills {
        let path = materialize(skills)?;
        params
            .context_files
            .get_or_insert_with(Default::default)
            .read
            .push(path);
    }
    Some(params)
}

pub(crate) fn build_resume(
    provider: &dyn AgentProvider,
    params: &crate::agent::provider::AgentResumeParams,
    skills: Option<&str>,
) -> Option<String> {
    let params = prepare_resume_context(
        params,
        skills,
        provider.is_tool_less(),
        provider.system_prompt_delivery(),
        provider.context_attach(),
        codeterm_util::task_files::write,
    )?;
    provider.build_resume_command_with_context(&params)
}

fn prepare_resume_context(
    params: &crate::agent::provider::AgentResumeParams,
    skills: Option<&str>,
    tool_less: bool,
    delivery: crate::agent::provider::SystemPromptDelivery,
    capability: Option<ContextAttach>,
    mut materialize: impl FnMut(&str, Option<&str>) -> Option<String>,
) -> Option<crate::agent::provider::AgentResumeParams> {
    let mut params = params.clone();
    if let Some(files) = params.context_files.take() {
        validate_support(&files, skills.is_some(), capability).ok()?;
        params.context_files = Some(resolve_files(Path::new(params.cwd.as_deref()?), files).ok()?);
    }
    if let Some(skills) = skills {
        validate_support(&TaskFiles::default(), true, capability).ok()?;
        params
            .context_files
            .get_or_insert_with(Default::default)
            .read
            .push(materialize(skills, Some(".plan-skills.md"))?);
    }
    if tool_less {
        match delivery {
            crate::agent::provider::SystemPromptDelivery::External => {
                params.tool_less_instructions_path = Some(materialize(
                    crate::prompts::TOOL_LESS,
                    Some(".tool-less.md"),
                )?);
            }
            _ => {
                params.system_prompt = Some(params.system_prompt.map_or_else(
                    || crate::prompts::TOOL_LESS.to_string(),
                    |prompt| format!("{}\n\n{prompt}", crate::prompts::TOOL_LESS),
                ));
            }
        }
    }
    Some(params)
}

#[cfg(test)]
mod tests;
