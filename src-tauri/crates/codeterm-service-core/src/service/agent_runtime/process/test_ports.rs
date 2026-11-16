use super::Process;
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

type Factory = Arc<dyn Fn() -> Process + Send + Sync>;
static FACTORIES: LazyLock<Mutex<HashMap<String, Factory>>> = LazyLock::new(Default::default);
type Spawn = (HashMap<String, String>, String);
static SPAWNED: LazyLock<Mutex<HashMap<String, Spawn>>> = LazyLock::new(Default::default);

pub(super) fn record_spawn(tab: &str, env: &HashMap<String, String>, cwd: &str) {
    SPAWNED.lock().insert(tab.into(), (env.clone(), cwd.into()));
}

/// The environment the host composed for the tab's latest fixture process spawn.
pub fn spawned_env(tab: &str) -> HashMap<String, String> {
    SPAWNED
        .lock()
        .get(tab)
        .map(|(env, _)| env.clone())
        .unwrap_or_default()
}

/// The working directory of the tab's latest fixture process spawn.
pub fn spawned_cwd(tab: &str) -> Option<String> {
    SPAWNED.lock().get(tab).map(|(_, cwd)| cwd.clone())
}

pub struct Registration(String);

pub fn connected() -> (Process, tokio::io::DuplexStream) {
    let (host, peer) = tokio::io::duplex(16384);
    let (reader, writer) = tokio::io::split(host);
    (
        Process::for_test(super::Client::attach(reader, writer)),
        peer,
    )
}

pub fn register(tab: &str, factory: impl Fn() -> Process + Send + Sync + 'static) -> Registration {
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
        SPAWNED.lock().remove(&self.0);
    }
}
