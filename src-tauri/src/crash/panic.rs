//! Rust panic → readable text report on disk.

use std::fmt::Write;

use super::report;

pub fn install_panic_hook() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        crate::keepawake::cleanup_active_children_for_exit();
        write_panic_report(info);
        prev(info);
    }));
}

fn write_panic_report(info: &std::panic::PanicHookInfo<'_>) {
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
    // The native sidecar embeds the log tail, so a panic that precedes an abort is
    // only correlatable if it reaches the log — writing a file alone leaves the
    log::error!("[crash] panic at {}: {}", location, payload);
    for line in payload.lines() {
        let _ = writeln!(s, "  {}", line);
    }
    let _ = writeln!(s);

    let bt = std::backtrace::Backtrace::force_capture();
    let _ = writeln!(s, "Backtrace:");
    for line in bt.to_string().lines() {
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

fn panic_payload(info: &std::panic::PanicHookInfo<'_>) -> String {
    let payload = info.payload();
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        return (*s).to_string();
    }
    if let Some(s) = payload.downcast_ref::<String>() {
        return s.clone();
    }
    format!("<non-string panic payload of type {:?}>", payload.type_id())
}
