use super::{capability, storage, State};
use crate::service::ServiceContext;

/// The caller owns provisioning and has confirmed every process using this root has exited.
pub(super) fn discard(
    svc: &ServiceContext,
    provider: &str,
    account: &str,
    namespace: &str,
) -> Result<(), String> {
    let state = svc.ext_or_init::<State>(Default::default);
    let capability =
        capability(svc, provider).ok_or("The account storage declaration is unavailable")?;
    let context = storage::provision_namespace(
        &state.root,
        provider,
        account,
        namespace,
        &capability,
        &storage::usual_home(&capability)?,
    )?;
    let declaration = svc
        .agent_registry
        .with_provider(provider, |provider| provider.agent_runtime())
        .flatten();
    crate::async_runtime::block_on(crate::service::plugin_host::cleanup_credential_secrets(
        svc,
        &context,
        declaration.as_ref(),
    ))?;
    storage::discard(&state.root, provider, namespace)
}
