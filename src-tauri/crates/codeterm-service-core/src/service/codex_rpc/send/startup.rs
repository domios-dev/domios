use super::*;

pub(crate) fn admit_startup(svc: &ServiceContext, tab: &str) -> Result<String, DeliveryError> {
    svc.chat_send_ledger
        .try_start_scoped_with_normalization(tab, "", None, None, Default::default())
        .map(|status| status.send_id)
        .map_err(|detail| DeliveryError::TabBusyRefused {
            tab: tab.into(),
            detail,
        })
}

pub(crate) fn send_startup(
    svc: &ServiceContext,
    tab: &str,
    text: &str,
    send_id: &str,
) -> Result<(), DeliveryError> {
    let reserved = svc
        .chat_send_ledger
        .status(send_id)
        .is_some_and(|status| status.tab_id == tab);
    startup_admission(reserved, svc.chat_send_ledger.has_rpc_handoff(send_id)).map_err(
        |detail| DeliveryError::SubmitUnconfirmed {
            tab: tab.into(),
            detail: detail.into(),
        },
    )?;
    if !super::super::lifecycle::startup_attachment_ready(tab) {
        return Err(DeliveryError::TabBusyRefused {
            tab: tab.into(),
            detail: "startup remote attachment pending".into(),
        });
    }
    let sidecar =
        super::super::lifecycle::for_tab(tab).ok_or_else(|| DeliveryError::TabBusyRefused {
            tab: tab.into(),
            detail: "startup sidecar unavailable".into(),
        })?;
    let thread = sidecar.thread.lock().clone();
    let binding = {
        let mux = svc.mux.read();
        mux.get_tab(tab).and_then(|tab| {
            (sidecar.verified.load(Ordering::Acquire)
                && tab.launch_session_authenticated()
                && tab.agent_session_id.as_deref() == Some(&thread))
            .then(|| tab.session_binding_nonce.clone())
            .flatten()
        })
    };
    let nonce = binding.ok_or_else(|| DeliveryError::TabBusyRefused {
        tab: tab.into(),
        detail: "startup bound-thread authority pending".into(),
    })?;
    if !svc.chat_send_ledger.bind_rpc_send(
        send_id,
        SendLiveBinding {
            session_id: Some(thread.clone()),
            binding_nonce: Some(nonce),
        },
    ) {
        return Err(DeliveryError::SubmitUnconfirmed {
            tab: tab.into(),
            detail: "startup RPC claim already consumed; replay refused".into(),
        });
    }
    observe(svc, tab, &sidecar);
    let cwd = svc.mux.read().get_tab(tab).and_then(|tab| tab.cwd.clone());
    let params = start_params(
        &thread,
        send_id,
        text,
        cwd.as_deref(),
        super::super::model::turn_overrides(tab),
    );
    let result =
        crate::async_runtime::block_on(submit(Origin::Agent, None, params, |method, params| {
            sidecar.client.request(method, params)
        }));
    finish_handoff(svc, tab, &thread, send_id, result)
}

fn startup_admission(reserved: bool, claimed: bool) -> Result<(), &'static str> {
    if claimed {
        Err("startup RPC handoff already claimed; replay refused")
    } else if !reserved {
        Err("startup reservation absent; replay refused")
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_rpc_send_startup_requires_reservation_and_refuses_every_claimed_replay() {
        assert!(startup_admission(true, false).is_ok());
        assert!(startup_admission(false, false).is_err());
        assert!(startup_admission(true, true).is_err());
        assert!(startup_admission(false, true).is_err());
    }

    #[tokio::test]
    async fn codex_rpc_send_startup_is_one_queued_turn_with_exact_id_and_overrides() {
        let params = start_params(
            "thread",
            "reserved-id",
            "task",
            Some("/cwd"),
            json!({"model":"chosen","effort":"high","approvalsReviewer":"user"}),
        );
        let mut calls = Vec::new();
        let result = submit(Origin::Agent, Some("active"), params, |method, params| {
            calls.push((method, params));
            std::future::ready(Ok(json!({"turn":{"startedAt":null}})))
        })
        .await;
        assert!(result.is_ok());
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, "turn/start");
        assert_eq!(calls[0].1["clientUserMessageId"], "reserved-id");
        assert_eq!(calls[0].1["input"], json!([{"type":"text","text":"task"}]));
        assert_eq!(calls[0].1["model"], "chosen");
        assert_eq!(calls[0].1["effort"], "high");
        assert_eq!(calls[0].1["approvalsReviewer"], "user");
    }
}
