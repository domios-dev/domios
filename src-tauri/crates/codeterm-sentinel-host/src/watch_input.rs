use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use codeterm_blueprint_types::sentinel_input::{InputSpec, MediaKind};
use codeterm_plugin::converter::ConverterRegistry;
use codeterm_service_core::service::ServiceContext;

use crate::{input, probe, store::Sentinel};

pub(crate) fn validate(svc: &ServiceContext, job: &Sentinel) -> Result<(), String> {
    let Some(spec) = job.input() else {
        return Ok(());
    };
    validate_spec(svc.ext::<Arc<ConverterRegistry>>(), spec)
}

fn validate_spec(registry: &ConverterRegistry, spec: &InputSpec) -> Result<(), String> {
    match spec.convert_to {
        Some(MediaKind::Text) => registry.plan(spec.kind, MediaKind::Text)
            .map(|_| ())
            .map_err(|error| format!("input_runtime_unavailable: {error}")),
        None if spec.kind == MediaKind::Text => Ok(()),
        _ => Err("input_runtime_unavailable: watch inputs must produce text; set convert_to: text and install a converter".into()),
    }
}

pub(crate) fn output(
    svc: &ServiceContext,
    job: &Sentinel,
    deadline: Instant,
    cancellation: &probe::cancellation::Cancellation,
) -> Result<probe::Output, String> {
    let spec = job.input().ok_or("input source missing")?;
    if spec.convert_to.is_none() {
        return input::text_output(job, deadline);
    }
    convert_output(
        svc.ext::<Arc<ConverterRegistry>>(),
        &codeterm_service_core::paths::data_dir(),
        &job.id,
        spec,
        deadline,
        cancellation,
    )
}

fn convert_output(
    registry: &ConverterRegistry,
    data_dir: &Path,
    id: &str,
    spec: &InputSpec,
    deadline: Instant,
    cancellation: &probe::cancellation::Cancellation,
) -> Result<probe::Output, String> {
    validate_spec(registry, spec)?;
    let run_id = uuid::Uuid::new_v4().simple().to_string();
    let run_dir = codeterm_blueprint_types::sentinel_input::run_dir(data_dir, id, &run_id)?;
    let cleanup = Arc::new(input::Cleanup::new(run_dir.clone()));
    let files = input::prepare(data_dir, id, &run_id, spec, deadline)?;
    let text = input::convert(
        registry,
        &files.source,
        &run_dir,
        spec.kind,
        MediaKind::Text,
        deadline,
        cancellation.conversion_token(),
        Some(cleanup),
        Instant::now,
    )?;
    Ok(probe::Output {
        bytes: text.as_bytes().to_vec(),
        text,
        success: true,
        exit_code: Some(0),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests;
