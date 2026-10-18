use super::{NativeHostClient, NativeLocalOutcome};
use codeterm_types::{
    app_quit::{QuitClient, QuitRefusal, QuitSnapshot},
    native_host::NativeHostResult,
    server_bootstrap::ServerReady,
};
use std::sync::atomic::Ordering;

pub(super) fn matches(
    client: &NativeHostClient,
    expected: &QuitSnapshot,
    ready: &ServerReady,
) -> bool {
    !client.retired.load(Ordering::Acquire)
        && client.server_process.provenance().ok().as_ref() == Some(&expected.ready.process)
        && expected.matches(&QuitSnapshot {
            ready: ready.clone(),
            client: Some(QuitClient {
                connection: client.connection.clone(),
                process: client.client_process.clone(),
            }),
        })
}

pub(super) fn dispatch(client: &NativeHostClient, expected: QuitSnapshot) -> NativeLocalOutcome {
    let result = if matches(client, &expected, &client.ready) {
        crate::run::request_client_quit_exact(&client.app, Some(expected))
    } else {
        Err(QuitRefusal::IdentityMismatch)
    };
    NativeLocalOutcome::Completed(match result {
        Ok(()) => NativeHostResult::ExactQuitAdmitted,
        Err(reason) => NativeHostResult::ExactQuitRefused(reason),
    })
}
