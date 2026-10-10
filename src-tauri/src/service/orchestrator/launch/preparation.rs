use crate::service::ServiceContext;
use std::sync::mpsc::{self, Receiver};

pub(super) struct Preparation(Receiver<String>);

pub(super) fn start(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    command: String,
) -> Preparation {
    let (done, result) = mpsc::sync_channel(1);
    let (svc, tab, provider) = (svc.clone(), tab.to_owned(), provider.to_owned());
    std::thread::spawn(move || {
        let command = crate::service::codex_rpc::launch::prepare(&svc, &tab, &provider, command);
        let _ = done.send(command);
    });
    Preparation(result)
}

impl Preparation {
    pub(super) fn finish(self) -> Result<String, String> {
        self.0
            .recv_timeout(std::time::Duration::from_secs(60))
            .map_err(|error| format!("Sidecar preparation unavailable: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_rpc_instant_start_preparation_uses_completion_not_elapsed_time() {
        let (done, result) = mpsc::sync_channel(1);
        assert!(matches!(result.try_recv(), Err(mpsc::TryRecvError::Empty)));
        done.send("opaque-command".to_owned()).unwrap();
        assert_eq!(Preparation(result).finish().unwrap(), "opaque-command");
    }

    #[test]
    fn codex_rpc_instant_start_failed_preparation_never_becomes_a_command() {
        let (done, result) = mpsc::sync_channel(1);
        drop(done);
        assert!(Preparation(result).finish().is_err());
    }
}
