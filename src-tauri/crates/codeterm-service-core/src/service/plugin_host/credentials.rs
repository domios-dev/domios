use std::collections::BTreeSet;
use std::time::Duration;

use codeterm_plugin::agent_runtime::AgentRuntimeCapability;
use codeterm_plugin::provider_account::{secret_reference, InvocationContext};

use crate::secrets::secret_file::SecretFile;
use crate::secrets::{SecretError, SecretStore};
use crate::service::ServiceContext;

pub async fn cleanup_credential_secrets(
    svc: &ServiceContext,
    context: &InvocationContext,
    declaration: Option<&AgentRuntimeCapability>,
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let store = tokio::time::timeout_at(deadline, svc.secrets.read())
        .await
        .map_err(|_| "Account credential store is busy")?
        .clone();
    let provider = context.provider().to_owned();
    let bucket = tokio::time::timeout_at(
        deadline,
        crate::async_runtime::spawn_blocking(move || super::secret_bucket(&provider)),
    )
    .await
    .map_err(|_| "Account secret bucket preparation timed out")?
    .map_err(|_| "Account secret bucket preparation failed")?;
    cleanup_with(store.as_ref(), bucket, context, declaration, deadline).await
}

async fn cleanup_with(
    store: &dyn SecretStore,
    bucket: SecretFile,
    context: &InvocationContext,
    declaration: Option<&AgentRuntimeCapability>,
    deadline: tokio::time::Instant,
) -> Result<(), String> {
    if !bucket.is_available() {
        return Err("Account secret bucket storage is unavailable".into());
    }
    let prefix = context
        .credential_secret_prefix(context.provider())
        .ok_or("Credential cleanup requires a declared selected home")?;
    let mut references = BTreeSet::new();
    for profile in declaration
        .and_then(|declaration| declaration.launch_profiles.as_ref())
        .into_iter()
        .flat_map(|profiles| profiles.values())
    {
        for reference in profile
            .auth_env
            .values()
            .map(|entry| &entry.secret)
            .chain(profile.secret_env.values())
        {
            if let Some(reference) = secret_reference(context.provider(), reference, Some(context))
            {
                references.insert(reference);
            }
            if references.len() > 128 {
                return Err("Credential cleanup declaration limit exceeded".into());
            }
        }
    }
    tokio::time::timeout_at(deadline, async {
        for reference in references {
            match store.delete(&reference).await {
                Ok(()) | Err(SecretError::NotFound) => (),
                Err(_) => {
                    return Err("Declared account credentials could not be deleted".to_owned())
                }
            }
        }
        Ok(())
    })
    .await
    .map_err(|_| "Account credential cleanup timed out")??;
    let deletion = crate::async_runtime::spawn_blocking(move || bucket.delete_prefix(&prefix));
    tokio::time::timeout_at(deadline, deletion)
        .await
        .map_err(|_| "Account secret bucket cleanup timed out")?
        .map_err(|_| "Account secret bucket cleanup worker failed")?
        .map_err(|_| "Account secret bucket could not be deleted")?;
    Ok(())
}

#[cfg(test)]
mod tests;
