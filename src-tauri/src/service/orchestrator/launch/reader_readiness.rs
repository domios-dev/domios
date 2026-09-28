use super::failure::LaunchFailure;

pub(super) enum ReaderReadiness {
    Ready,
    Pending {
        startup: std::sync::mpsc::Receiver<Result<(), LaunchFailure>>,
        closed: std::sync::mpsc::Receiver<()>,
    },
}

pub(super) enum ReaderMonitor {
    Active(std::sync::mpsc::Receiver<()>),
    Unavailable,
}

impl ReaderMonitor {
    pub(super) fn closed(&self) -> bool {
        match self {
            Self::Active(rx) => !matches!(rx.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)),
            Self::Unavailable => false,
        }
    }
}

impl ReaderReadiness {
    pub(super) fn wait(self, timeout: std::time::Duration) -> Result<ReaderMonitor, LaunchFailure> {
        match self {
            Self::Ready => Ok(ReaderMonitor::Unavailable),
            Self::Pending { startup, closed } => match startup.recv_timeout(timeout) {
                Ok(Ok(())) => Ok(ReaderMonitor::Active(closed)),
                Ok(Err(error)) => Err(error),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    Err(LaunchFailure::ReaderReadyTimeout)
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(
                    LaunchFailure::ReaderRuntime("reader startup channel closed".into()),
                ),
            },
        }
    }
}

pub(super) struct BackendPty {
    pub(super) id: String,
    pub(super) reader: ReaderReadiness,
}
