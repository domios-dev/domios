use std::cell::RefCell;

use super::run_guarded_hook;

#[test]
fn panic_hook_reentrancy_skips_nested_report_callbacks() {
    let events = RefCell::new(Vec::new());
    run_guarded_hook(
        || {
            events.borrow_mut().push("diagnostics");
            run_guarded_hook(
                || events.borrow_mut().push("nested diagnostics"),
                |_| events.borrow_mut().push("nested report"),
                || events.borrow_mut().push("fallback"),
            );
        },
        |_| {
            run_guarded_hook(
                || events.borrow_mut().push("nested diagnostics"),
                |_| events.borrow_mut().push("nested cleanup"),
                || events.borrow_mut().push("fallback before cleanup"),
            );
            events.borrow_mut().push("cleanup");
            events.borrow_mut().push("logging");
            events.borrow_mut().push("report");
            events.borrow_mut().push("previous hook");
        },
        || events.borrow_mut().push("unexpected fallback"),
    );
    assert_eq!(
        *events.borrow(),
        [
            "diagnostics",
            "fallback",
            "fallback before cleanup",
            "cleanup",
            "logging",
            "report",
            "previous hook"
        ]
    );
    run_guarded_hook(
        || events.borrow_mut().push("next diagnostics"),
        |_| events.borrow_mut().push("next report"),
        || events.borrow_mut().push("unexpected fallback"),
    );
    assert_eq!(&events.borrow()[7..], ["next diagnostics", "next report"]);
}

#[test]
fn panic_hook_reentrancy_resets_after_callback_unwind() {
    let result = std::panic::catch_unwind(|| {
        run_guarded_hook(|| (), |_| panic!("controlled callback failure"), || ());
    });
    assert!(result.is_err());
    let handled = std::cell::Cell::new(false);
    run_guarded_hook(|| (), |_| handled.set(true), || ());
    assert!(handled.get());
}

#[test]
fn panic_hook_reentrancy_is_thread_local() {
    run_guarded_hook(
        || (),
        |_| {
            let handled = std::thread::spawn(|| {
                let handled = std::cell::Cell::new(false);
                run_guarded_hook(|| (), |_| handled.set(true), || ());
                handled.get()
            })
            .join()
            .unwrap();
            assert!(handled);
        },
        || panic!("unexpected fallback"),
    );
}

#[test]
fn panic_hook_reentrancy_fallback_ignores_output_failure() {
    struct FailedWriter(usize);

    impl std::io::Write for FailedWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            self.0 += 1;
            Err(std::io::ErrorKind::BrokenPipe.into())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut writer = FailedWriter(0);
    run_guarded_hook(
        || (),
        |_| {
            run_guarded_hook(
                || panic!("unexpected diagnostics"),
                |_: ()| panic!("unexpected report"),
                || super::write_reentry_diagnostic_to(&mut writer),
            );
        },
        || panic!("unexpected fallback"),
    );
    assert_eq!(writer.0, 1);
}
