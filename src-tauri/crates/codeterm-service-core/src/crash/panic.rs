//! Rust panic → readable text report on disk.

use std::cell::Cell;
use std::fmt::Write;

use super::report;

thread_local! {
    static IN_PANIC_HOOK: Cell<bool> = const { Cell::new(false) };
}

struct PanicHookGuard;

impl PanicHookGuard {
    fn enter() -> Option<Self> {
        IN_PANIC_HOOK
            .try_with(|active| (!active.replace(true)).then(|| Self))
            .ok()
            .flatten()
    }
}

impl Drop for PanicHookGuard {
    fn drop(&mut self) {
        let _ = IN_PANIC_HOOK.try_with(|active| active.set(false));
    }
}

fn run_guarded_hook<T>(
    diagnostics: impl FnOnce() -> T,
    handle: impl FnOnce(T),
    fallback: impl FnOnce(),
) {
    let Some(_guard) = PanicHookGuard::enter() else {
        fallback();
        return;
    };
    let diagnostic = diagnostics();
    handle(diagnostic);
}

pub fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        run_guarded_hook(
            || write_initial_diagnostics(info),
            |backtrace| {
                crate::keepawake::cleanup_active_children_for_exit();
                write_panic_report(info, &backtrace);
                prev(info);
            },
            write_reentry_diagnostic,
        );
    }));
}

fn write_reentry_diagnostic() {
    write_reentry_diagnostic_to(&mut std::io::stderr());
}

fn write_reentry_diagnostic_to(stderr: &mut impl std::io::Write) {
    let _ = stderr.write_all(b"\n[crash] panic hook re-entry; skipping report machinery\n");
}

fn write_initial_diagnostics(info: &std::panic::PanicHookInfo<'_>) -> std::backtrace::Backtrace {
    use std::io::Write as _;

    let mut stderr = std::io::stderr();
    let _ = writeln!(stderr, "\n[crash] panic: {}", panic_payload(info));
    if let Some(location) = info.location() {
        let _ = writeln!(stderr, "Location: {}", location);
    }
    let thread = std::thread::current();
    let _ = writeln!(
        stderr,
        "Thread: {} ({:?})",
        thread.name().unwrap_or("<unnamed>"),
        thread.id()
    );
    let backtrace = std::backtrace::Backtrace::force_capture();
    let _ = writeln!(stderr, "Backtrace:\n{}", backtrace);
    backtrace
}

fn write_panic_report(info: &std::panic::PanicHookInfo<'_>, backtrace: &std::backtrace::Backtrace) {
    let ts = report::timestamp_slug();
    let path = report::crashes_dir().join(format!("crash-{}-panic.txt", ts));

    let mut s = report::header("panic");
    let _ = writeln!(s);

    let _ = writeln!(
        s,
        "Thread: {}",
        std::thread::current().name().unwrap_or("<unnamed>")
    );

    let location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "<unknown>".into());
    let _ = writeln!(s, "Location: {}", location);
    let _ = writeln!(s);

    let _ = writeln!(s, "Panic message:");
    let payload = panic_payload(info);
    // The native sidecar embeds the log tail, so a panic that precedes an abort is only correlatable if it reaches the log — writing a file alone leaves the abort report showing no panic at all.
    log::error!("[crash] panic at {}: {}", location, payload);
    for line in payload.lines() {
        let _ = writeln!(s, "  {}", line);
    }
    let _ = writeln!(s);

    let _ = writeln!(s, "Backtrace:");
    for line in backtrace.to_string().lines() {
        let _ = writeln!(s, "  {}", line);
    }
    let _ = writeln!(s);

    let _ = writeln!(s, "Recent log tail (last 200 lines):");
    let _ = writeln!(s, "---");
    let _ = writeln!(s, "{}", report::log_tail(200));

    let _ = std::fs::write(&path, &s);
    let _ = std::io::Write::write_all(
        &mut std::io::stderr(),
        format!("\n[crash] panic written to {}\n", path.display()).as_bytes(),
    );
}

fn panic_payload<'a>(info: &'a std::panic::PanicHookInfo<'_>) -> &'a str {
    let payload = info.payload();
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        return s;
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.as_str();
    }
    "<non-string panic payload>"
}

#[cfg(test)]
mod tests;
