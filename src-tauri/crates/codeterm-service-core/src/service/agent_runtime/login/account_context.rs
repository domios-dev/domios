use super::*;
use crate::multiplexer::types::{TabBacking, TabType};

#[derive(Clone)]
pub(crate) struct OpenedContext {
    pub tab: String,
    pub initialized: Option<tokio::sync::watch::Receiver<bool>>,
    provider: String,
    account: String,
    initial_nonce: String,
    nonce: Arc<Mutex<String>>,
}

pub(crate) fn open(
    svc: &ServiceContext,
    provider: &str,
    account: &str,
) -> Result<OpenedContext, String> {
    let adapter = svc
        .agent_registry
        .provider_arc(provider)
        .ok_or("Harness is unavailable")?;
    let invocation = crate::service::provider_accounts::invocation(svc, provider, Some(account))?;
    let runtime = adapter.agent_runtime().is_some();
    if !runtime && adapter.cli_login().is_none() {
        return Err("This harness has no account sign-in method".into());
    }
    let capability = adapter
        .account_capability()
        .ok_or("This harness does not support isolated accounts")?;
    let cwd = match invocation.as_ref() {
        Some(context) => context
            .environment_entries()
            .find(|(name, _)| *name == capability.home_environment)
            .and_then(|(_, value)| value.map(str::to_owned))
            .ok_or("The selected account home is unavailable")?,
        None => {
            let usual = crate::service::provider_accounts::usual_home_for_login(&capability)?;
            if std::path::Path::new(&usual).is_dir() {
                usual
            } else {
                dirs::home_dir()
                    .ok_or("The sign-in working directory is unavailable")?
                    .to_string_lossy()
                    .into_owned()
            }
        }
    };
    let nonce = uuid::Uuid::new_v4().to_string();
    let tab = {
        let mut mux = svc.mux.write();
        let workspace = mux
            .get_active_workspace()
            .map(|workspace| workspace.id.clone())
            .or_else(|| {
                mux.list_workspaces()
                    .first()
                    .map(|workspace| workspace.id.clone())
            })
            .ok_or("A workspace is required for account sign-in")?;
        let tab = mux.create_tab(&workspace)?;
        let target = mux
            .get_tab_mut(&tab)
            .ok_or("The sign-in context is unavailable")?;
        target.tab_type = TabType::Choice;
        target.hidden = true;
        target.show_in_tabbar = false;
        target.skip_snapshot = true;
        target.agent_type = Some(provider.into());
        target.cwd = Some(cwd.into());
        target.description = Some("domios:provider-account-sign-in".into());
        target.restorable_launch.account_id = Some(account.into());
        target.session_binding_nonce = Some(nonce.clone());
        target.agent_runtime_state = Some(
            json!({"state":"loading","epoch":0,"auth":"unknown","loginBinding":{"nonce":nonce,"epoch":0}}),
        );
        if runtime {
            target.backing = TabBacking::ChatBackend {
                plugin_id: provider.into(),
                session_id: String::new(),
            };
        }
        tab
    };
    let initialized = if runtime {
        if let Err(error) = super::super::authority::record_spawn(svc, &tab, provider, &nonce) {
            let _ = svc.close_tab(&tab);
            return Err(error);
        }
        let owner = svc.clone();
        let target = tab.clone();
        let binding = nonce.clone();
        let (completed, initialized) = tokio::sync::watch::channel(false);
        crate::async_runtime::spawn(async move {
            let completion = Completion(completed);
            if let Err(error) = engine::ensure(&owner, &target).await {
                if !matches!(error, RuntimeFailure::SignInRequired) {
                    if let Some(tab) = owner.mux.write().get_tab_mut(&target).filter(|tab| {
                        tab.session_binding_nonce.as_deref() == Some(binding.as_str())
                    }) {
                        let epoch = tab
                            .agent_runtime_state
                            .as_ref()
                            .and_then(|state| state["epoch"].as_u64())
                            .unwrap_or(0);
                        tab.agent_runtime_state = Some(
                            json!({"state":"degraded","epoch":epoch,"auth":"unknown","reason":"Account sign-in could not start"}),
                        );
                    }
                    crate::service::provider_accounts::login_changed(&owner, &target);
                }
            }
            completion.complete();
        });
        Some(initialized)
    } else {
        let value = match cli::show(svc, &tab) {
            Ok(value) => value,
            Err(error) => {
                let _ = svc.close_tab(&tab);
                return Err(error);
            }
        };
        if let Some(target) = svc.mux.write().get_tab_mut(&tab) {
            target.agent_runtime_state = value.get("state").cloned();
        }
        None
    };
    Ok(OpenedContext {
        tab,
        initialized,
        provider: provider.into(),
        account: account.into(),
        initial_nonce: nonce.clone(),
        nonce: Arc::new(Mutex::new(nonce)),
    })
}

pub(crate) async fn close(svc: &ServiceContext, context: &OpenedContext) -> Result<(), String> {
    let tab = &context.tab;
    {
        let mut nonce = context.nonce.lock();
        let is_context = svc.mux.read().get_tab(tab).is_some_and(|target| {
            target.description.as_deref() == Some("domios:provider-account-sign-in")
                && target.hidden
                && target.skip_snapshot
                && target.agent_type.as_deref() == Some(context.provider.as_str())
                && crate::service::provider_accounts::selected_account(target) == context.account
                && target.session_binding_nonce.as_deref() == Some(nonce.as_str())
        });
        if !is_context {
            return Err("The account sign-in context changed".into());
        }
        *nonce = svc.rotate_session_binding_nonce(tab);
    }
    cli::close_and_wait(
        tab,
        &context.provider,
        &context.account,
        Some(&context.initial_nonce),
    )
    .await?;
    if let Some(mut initialized) = context.initialized.clone() {
        tokio::time::timeout(Duration::from_secs(30), initialized.wait_for(|done| *done))
            .await
            .map_err(|_| "The sign-in runtime is still starting; credentials were retained")?
            .map_err(|_| "Runtime startup cleanup is unconfirmed; credentials were retained")?;
    }
    engine::stop(svc, tab).await?;
    svc.close_tab_async(tab).await
}
