//! Update teardown ordering seam. Enforces the invariant that viability is
//! proven BEFORE any destructive PTY teardown, and that any failure once
//! teardown has begun leaves a diagnostic artifact + user-visible signal.

use std::future::Future;

use super::failure_report::{UpdateAbort, UpdateFailureSink};

/// Order: viability -> shutdown -> install.
///
/// - Defect 1: `viability` runs first; if it fails, `shutdown` is NEVER invoked
///   and the update aborts with panes still alive.
/// - Defect 4: once `shutdown` starts, teardown is underway, so a failure in
///   either `shutdown` or `install` routes through `sink` (artifact + signal).
///
/// `shutdown` returns the tab ids it tore down so the abort artifact can name
/// them.
pub async fn install_after_shutdown<V, VF, S, SF, I, IF>(
    verified_bytes: Vec<u8>,
    viability: V,
    shutdown: S,
    install: I,
    sink: &dyn UpdateFailureSink,
) -> Result<(), String>
where
    V: FnOnce() -> VF,
    VF: Future<Output = Result<(), String>>,
    S: FnOnce() -> SF,
    SF: Future<Output = Result<Vec<String>, String>>,
    I: FnOnce(Vec<u8>) -> IF,
    IF: Future<Output = Result<(), String>>,
{
    viability().await?;

    let torn_down = match shutdown().await {
        Ok(tabs) => tabs,
        Err(cause) => {
            sink.abort(&UpdateAbort::new("shutdown", &cause, Vec::new()));
            return Err(cause);
        }
    };

    match install(verified_bytes).await {
        Ok(()) => Ok(()),
        Err(cause) => {
            sink.abort(&UpdateAbort::new("install", &cause, torn_down));
            Err(cause)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct SpySink(Arc<Mutex<Vec<UpdateAbort>>>);
    impl UpdateFailureSink for SpySink {
        fn abort(&self, abort: &UpdateAbort) {
            self.0.lock().unwrap().push(abort.clone());
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn viability_precedes_shutdown_and_install() {
        let events = Arc::new(Mutex::new(vec!["download"]));
        let (v, s, i) = (events.clone(), events.clone(), events.clone());
        let sink = SpySink::default();

        install_after_shutdown(
            vec![1, 2, 3],
            move || async move {
                v.lock().unwrap().push("viability");
                Ok(())
            },
            move || async move {
                s.lock().unwrap().push("shutdown");
                Ok(vec!["w-1".to_string()])
            },
            move |bytes| async move {
                assert_eq!(bytes, [1, 2, 3]);
                i.lock().unwrap().push("install");
                Ok(())
            },
            &sink,
        )
        .await
        .unwrap();

        assert_eq!(
            *events.lock().unwrap(),
            ["download", "viability", "shutdown", "install"]
        );
        assert!(sink.0.lock().unwrap().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn failed_viability_never_tears_down_panes() {
        let shutdown_ran = Arc::new(Mutex::new(false));
        let installed = Arc::new(Mutex::new(false));
        let (s, i) = (shutdown_ran.clone(), installed.clone());
        let sink = SpySink::default();

        let err = install_after_shutdown(
            vec![1],
            || async { Err("viability: install target not writable".to_string()) },
            move || {
                let s = s.clone();
                async move {
                    *s.lock().unwrap() = true;
                    Ok(Vec::new())
                }
            },
            move |_| async move {
                *i.lock().unwrap() = true;
                Ok(())
            },
            &sink,
        )
        .await
        .unwrap_err();

        assert!(err.contains("install target not writable"));
        assert!(
            !*shutdown_ran.lock().unwrap(),
            "panes were torn down anyway"
        );
        assert!(!*installed.lock().unwrap());
        // Aborting with panes alive is not a teardown-abort: no artifact/signal.
        assert!(sink.0.lock().unwrap().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn install_failure_after_teardown_raises_signal_with_torn_down_tabs() {
        let sink = SpySink::default();

        let err = install_after_shutdown(
            vec![1],
            || async { Ok(()) },
            || async { Ok(vec!["w-1".to_string(), "w-2".to_string()]) },
            |_| async { Err("installer launch failed: exit 1".to_string()) },
            &sink,
        )
        .await
        .unwrap_err();

        assert_eq!(err, "installer launch failed: exit 1");
        let aborts = sink.0.lock().unwrap();
        assert_eq!(aborts.len(), 1);
        assert_eq!(aborts[0].phase, "install");
        assert_eq!(aborts[0].torn_down_tabs, vec!["w-1", "w-2"]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn shutdown_failure_raises_signal() {
        let installed = Arc::new(Mutex::new(false));
        let i = installed.clone();
        let sink = SpySink::default();

        let err = install_after_shutdown(
            vec![1],
            || async { Ok(()) },
            || async { Err("PTY reap failed".to_string()) },
            move |_| async move {
                *i.lock().unwrap() = true;
                Ok(())
            },
            &sink,
        )
        .await
        .unwrap_err();

        assert_eq!(err, "PTY reap failed");
        assert!(!*installed.lock().unwrap());
        let aborts = sink.0.lock().unwrap();
        assert_eq!(aborts.len(), 1);
        assert_eq!(aborts[0].phase, "shutdown");
    }
}
