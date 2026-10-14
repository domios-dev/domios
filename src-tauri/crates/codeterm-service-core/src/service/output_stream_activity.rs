use tokio::sync::broadcast;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum OutputStreamActivity {
    Pty(String),
    Mux,
}

pub(crate) fn channel() -> broadcast::Sender<OutputStreamActivity> {
    broadcast::channel(4096).0
}

impl super::ServiceContext {
    pub(crate) fn output_stream_pty_changed(&self, pty_id: &str) {
        let _ = self
            .output_stream_activity
            .send(OutputStreamActivity::Pty(pty_id.to_string()));
    }

    pub(crate) fn output_stream_mux_changed(&self) {
        let _ = self.output_stream_activity.send(OutputStreamActivity::Mux);
    }
}
