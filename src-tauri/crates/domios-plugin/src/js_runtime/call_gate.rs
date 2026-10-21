//! Test-only deterministic gate. A plugin call parks on a named gate and the
//! test releases it by signalling an event — never by waiting out a duration.
//! Installed only in `cfg(test)` builds, so it adds no drop-in-reachable surface.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use parking_lot::{Condvar, Mutex};
use rquickjs::function::Func;
use rquickjs::Object;

use super::host_api::PluginSandbox;
use super::vm::VmError;

#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    cv: Condvar,
}

static GATES: LazyLock<Mutex<HashMap<String, Arc<Gate>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn gate(name: &str) -> Arc<Gate> {
    GATES.lock().entry(name.to_string()).or_default().clone()
}

/// Release everyone parked on `name`, and anyone who parks on it later.
pub(super) fn open(name: &str) {
    let g = gate(name);
    *g.open.lock() = true;
    g.cv.notify_all();
}

pub(super) fn is_open(name: &str) -> bool {
    *gate(name).open.lock()
}

pub(super) fn reset(name: &str) {
    GATES.lock().remove(name);
}

/// Park until `name` opens. Unbounded on purpose: the opener is the test.
fn wait(name: &str) {
    let g = gate(name);
    let mut open = g.open.lock();
    while !*open {
        g.cv.wait(&mut open);
    }
}

/// Park until `name` opens, giving up after `deadline`. The deadline is a
/// failure boundary for a wedged expectation, never evidence that it succeeded —
/// `false` means the test failed, not that enough time passed.
pub(super) fn wait_opened(name: &str, deadline: Duration) -> bool {
    let g = gate(name);
    let mut open = g.open.lock();
    g.cv.wait_while_for(&mut open, |o| !*o, deadline);
    *open
}

pub(super) fn install(host: &Object<'_>, sandbox: &Arc<PluginSandbox>) -> Result<(), VmError> {
    macro_rules! bind {
        ($name:expr, $closure:expr) => {
            host.set($name, Func::new($closure))
                .map_err(|e| VmError::Script(format!("set {}: {e}", $name)))?
        };
    }

    // Blocks inside the caller's JS frame — the pre-fix shape.
    bind!("testGateBlock", |name: String| -> String {
        wait(&name);
        "{}".to_string()
    });

    // Returns a job id whose completion is the gate opening — the awaitable shape.
    let sb = sandbox.clone();
    bind!("testGateStart", move |name: String| -> String {
        super::host_exec::start_gated_job(sb.plugin_id(), move || wait(&name))
    });

    bind!("testGateOpen", |name: String| { open(&name) });

    Ok(())
}
