use tokio::sync::broadcast;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputStreamActivity {
    Pty(String),
    Mux,
}

pub struct OutputStreamActivityTx(pub broadcast::Sender<OutputStreamActivity>);

pub fn channel() -> broadcast::Sender<OutputStreamActivity> {
    broadcast::channel(4096).0
}

impl super::ServiceContext {
    pub fn output_stream_pty_changed(&self, pty_id: &str) {
        let _ = self
            .ext::<crate::service::output_stream_activity::OutputStreamActivityTx>()
            .0
            .send(OutputStreamActivity::Pty(pty_id.to_string()));
    }

    pub(crate) fn output_stream_mux_changed(&self) {
        let _ = self
            .ext::<crate::service::output_stream_activity::OutputStreamActivityTx>()
            .0
            .send(OutputStreamActivity::Mux);
    }
}
