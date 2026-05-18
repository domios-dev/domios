//! Native crash (SIGSEGV / SIGABRT / stack overflow / Windows SEH) → minidump + readable sidecar.
//!
//! Two artifacts per crash:
//!   crash-<ts>-native.dmp — binary minidump (open with minidump_stackwalk, MSVC, Sentry)
//!   crash-<ts>-native.txt — human-readable sidecar (signal, version, log tail)
//!
//! The crash callback runs in a compromised context. We do the bare minimum:
//! open two files, write structured strings, write the minidump. No allocator
//! work beyond what the writers themselves do.

use std::fmt::Write;

use crash_handler::{make_crash_event, CrashEventResult, CrashHandler};

use super::report;

/// Install the crash handler. The returned handler must be kept alive for the
/// lifetime of the process — we `Box::leak` it from the caller.
pub fn install() -> Option<CrashHandler> {
    let event = unsafe {
        make_crash_event(|ctx: &crash_handler::CrashContext| {
            on_crash(ctx);
            CrashEventResult::Handled(false)
        })
    };

    match CrashHandler::attach(event) {
        Ok(h) => Some(h),
        Err(e) => {
            log::warn!("[crash] failed to attach native handler: {:?}", e);
            None
        }
    }
}

fn on_crash(ctx: &crash_handler::CrashContext) {
    let ts = report::timestamp_slug();
    let dir = report::crashes_dir();
    let dmp_path = dir.join(format!("crash-{}-native.dmp", ts));
    let txt_path = dir.join(format!("crash-{}-native.txt", ts));

    // Best-effort: write the binary minidump first.
    let dmp_ok = write_minidump(ctx, &dmp_path);

    // Always write the readable sidecar.
    let mut s = report::header("native");
    let _ = writeln!(s);
    let _ = writeln!(s, "Signal/Exception: {}", describe_ctx(ctx));
    let _ = writeln!(s);
    let _ = writeln!(s, "Minidump: {} ({})", dmp_path.display(), if dmp_ok { "ok" } else { "failed" });
    let _ = writeln!(s);
    let _ = writeln!(s, "Recent log tail (last 200 lines):");
    let _ = writeln!(s, "---");
    let _ = writeln!(s, "{}", report::log_tail(200));
    let _ = std::fs::write(&txt_path, &s);

    crash_handler::write_stderr("\n[crash] native crash written, see ~/.codeterm/crashes/\n");
}

#[cfg(target_os = "macos")]
fn write_minidump(ctx: &crash_handler::CrashContext, path: &std::path::Path) -> bool {
    use minidump_writer::minidump_writer::MinidumpWriter;
    let Ok(mut file) = std::fs::File::create(path) else { return false };
    // Rebuild the context (it's not Clone) into a value-owned form for the writer.
    let owned = crash_context::CrashContext {
        task: ctx.task,
        thread: ctx.thread,
        handler_thread: ctx.handler_thread,
        exception: ctx.exception,
    };
    MinidumpWriter::with_crash_context(owned).dump(&mut file).is_ok()
}

#[cfg(target_os = "windows")]
fn write_minidump(ctx: &crash_handler::CrashContext, path: &std::path::Path) -> bool {
    use minidump_writer::minidump_writer::MinidumpWriter;
    let Ok(mut file) = std::fs::File::create(path) else { return false };
    // MinidumpWriter::dump_local_context dumps the current process using the
    // provided exception pointers from crash-handler.
    MinidumpWriter::dump_local_context(
        Some(ctx.exception_code),
        None,
        ctx.exception_pointers as _,
        &mut file,
    ).is_ok()
}

#[cfg(target_os = "linux")]
fn write_minidump(ctx: &crash_handler::CrashContext, path: &std::path::Path) -> bool {
    use minidump_writer::minidump_writer::MinidumpWriter;
    let Ok(mut file) = std::fs::File::create(path) else { return false };
    MinidumpWriter::new(ctx.pid, ctx.tid).dump(&mut file).is_ok()
}

#[cfg(target_os = "macos")]
fn describe_ctx(ctx: &crash_handler::CrashContext) -> String {
    match ctx.exception {
        Some(e) => format!("kind=0x{:x} code=0x{:x} subcode={:?}", e.kind, e.code, e.subcode),
        None => "<no exception info>".to_string(),
    }
}

#[cfg(target_os = "windows")]
fn describe_ctx(ctx: &crash_handler::CrashContext) -> String {
    format!("exception_code=0x{:x}", ctx.exception_code)
}

#[cfg(target_os = "linux")]
fn describe_ctx(ctx: &crash_handler::CrashContext) -> String {
    format!("siginfo.si_signo={} si_code={}", ctx.siginfo.ssi_signo, ctx.siginfo.ssi_code)
}
