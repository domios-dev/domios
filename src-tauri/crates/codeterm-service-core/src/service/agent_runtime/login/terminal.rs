use super::{claim, engine, forget, ServiceContext, Session};
use crate::service::shell_exec::ExecWaiter;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

pub(super) async fn launch(
    svc: &ServiceContext,
    tab: &str,
    session: &Arc<Session>,
    command: Option<String>,
) -> Result<Value, String> {
    let command = command.ok_or("Terminal sign-in unsupported")?;
    let team = svc
        .mux
        .read()
        .get_tab(tab)
        .and_then(|target| target.team_id.clone())
        .ok_or("Terminal sign-in requires a Team utility shell")?;
    claim(session)?;
    let owner = svc.clone();
    let target = tab.to_string();
    let launched = tokio::task::spawn_blocking(move || {
        let handle = owner.spawn_tab(crate::service::spawn::SpawnTabSpec {
            creator_tab_id: Some(target),
            team_id: Some(team),
            title: Some("SignIn".into()),
            description: Some("Provider sign-in".into()),
            ..Default::default()
        })?;
        let prepared = (|| {
            owner.ensure_tab_pty_for_send(&handle.tab_id)?;
            let pty = owner.resolve_pty_id(&handle.tab_id)?;
            if !crate::service::pty_io::await_shell_prompt(
                &owner.screen_state,
                &pty,
                Duration::from_secs(30),
            ) {
                return Err("Sign-in shell did not become ready".to_string());
            }
            let waiter = ExecWaiter::register(&pty);
            owner.send_shell_spawn_after_ready(&pty, &command)?;
            Ok(waiter)
        })();
        match prepared {
            Ok(waiter) => Ok((handle.tab_id, waiter)),
            Err(error) => {
                let _ = owner.close_tab_with_reason(
                    &handle.tab_id,
                    crate::multiplexer::types::TabCloseReason::SpawnRollback,
                );
                Err(error)
            }
        }
    })
    .await;
    let (utility, waiter) = match launched {
        Ok(Ok(launched)) => launched,
        _ => {
            forget(session);
            return Err("Terminal sign-in failed".into());
        }
    };
    let owner = svc.clone();
    let session = session.clone();
    let target = tab.to_string();
    let shell = utility.clone();
    let expected = session.context.lock().clone();
    let task: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> =
        Box::pin(async move {
            let completion = waiter.wait(Duration::from_secs(600)).await;
            if completion.is_none() {
                let owner = owner.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    owner.close_tab_with_reason(
                        &shell,
                        crate::multiplexer::types::TabCloseReason::SpawnRollback,
                    )
                })
                .await;
            }
            forget(&session);
            if completion.and_then(|completion| completion.exit_code()) == Some(0)
                && session.context.lock().epoch == expected.epoch
                && engine::validate_binding(&owner, &session).is_ok()
            {
                let _ = engine::ensure(&owner, &target).await;
            }
        });
    crate::async_runtime::spawn(task);
    Ok(json!({"tabId":utility}))
}
