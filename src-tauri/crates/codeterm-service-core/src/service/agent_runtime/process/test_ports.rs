use super::Process;
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

type Factory = Arc<dyn Fn() -> Process + Send + Sync>;
static FACTORIES: LazyLock<Mutex<HashMap<String, Factory>>> = LazyLock::new(Default::default);
static SPAWNED_ENV: LazyLock<Mutex<HashMap<String, HashMap<String, String>>>> =
    LazyLock::new(Default::default);

pub(super) fn record_env(tab: &str, env: &HashMap<String, String>) {
    SPAWNED_ENV.lock().insert(tab.into(), env.clone());
}

/// The environment the host composed for the tab's latest fixture process spawn.
pub(crate) fn spawned_env(tab: &str) -> HashMap<String, String> {
    SPAWNED_ENV.lock().get(tab).cloned().unwrap_or_default()
}

pub(crate) struct Registration(String);

pub(crate) fn connected() -> (Process, tokio::io::DuplexStream) {
    let (host, peer) = tokio::io::duplex(16384);
    let (reader, writer) = tokio::io::split(host);
    (
        Process::for_test(super::Client::attach(reader, writer)),
        peer,
    )
}

pub(crate) fn register(
    tab: &str,
    factory: impl Fn() -> Process + Send + Sync + 'static,
) -> Registration {
    assert!(FACTORIES
        .lock()
        .insert(tab.into(), Arc::new(factory))
        .is_none());
    Registration(tab.into())
}

pub(super) fn factory(tab: &str) -> Option<Factory> {
    FACTORIES.lock().get(tab).cloned()
}

impl Drop for Registration {
    fn drop(&mut self) {
        FACTORIES.lock().remove(&self.0);
        SPAWNED_ENV.lock().remove(&self.0);
    }
}
