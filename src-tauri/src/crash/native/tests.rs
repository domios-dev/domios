use super::*;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

const ILLEGAL: i32 = 0xc000001du32 as i32;
const CHILD_MODE: &str = "CODETERM_NATIVE_PROBE_MODE";
const CHILD_DIR: &str = "CODETERM_NATIVE_PROBE_DIR";

#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentThreadId() -> u32;
    fn GetModuleHandleW(name: *const u16) -> *mut std::ffi::c_void;
    fn RaiseException(code: u32, flags: u32, count: u32, arguments: *const usize);
}

fn marker() {}

fn supplied_context(
    address: usize,
    thread_id: u32,
    test: impl FnOnce(&crash_context::CrashContext),
) {
    // SAFETY: these Windows records consist of integers/pointers and accept zero initialization.
    let mut record: crash_context::EXCEPTION_RECORD = unsafe { std::mem::zeroed() };
    let mut cpu = std::mem::MaybeUninit::zeroed();
    // SAFETY: capture_context initializes the correctly sized, aligned destination.
    unsafe { crash_context::capture_context(cpu.as_mut_ptr()) };
    // SAFETY: capture_context has initialized the CPU record.
    let mut cpu = unsafe { cpu.assume_init() };
    record.ExceptionCode = ILLEGAL;
    record.ExceptionAddress = address as *mut _;
    let pointers = crash_context::EXCEPTION_POINTERS {
        ExceptionRecord: &mut record,
        ContextRecord: &mut cpu,
    };
    test(&crash_context::CrashContext {
        exception_pointers: &pointers,
        exception_code: ILLEGAL,
        process_id: std::process::id(),
        thread_id,
    });
}

fn field<'a>(text: &'a str, name: &str) -> &'a str {
    text.split_once(&format!("{name}="))
        .unwrap()
        .1
        .split_whitespace()
        .next()
        .unwrap()
}

fn number(text: &str, name: &str) -> u64 {
    let value = field(text, name);
    if let Some(hex) = value.strip_prefix("0x") {
        u64::from_str_radix(hex, 16).unwrap()
    } else {
        value.parse().unwrap()
    }
}

#[test]
fn supplied_context_reaches_prewrite_and_failed_writer_record() {
    let dir = tempfile::tempdir().unwrap();
    let dmp = dir.path().join("probe.dmp");
    let txt = dir.path().join("probe.txt");
    let address = marker as *const () as usize;
    let tid = 0x13579;
    supplied_context(address, tid, |ctx| {
        let description = describe_ctx(ctx);
        let expected_module = windows_context::module_at(address).unwrap();
        // SAFETY: null requests the current executable's existing module handle.
        let image_base = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
        assert_ne!(image_base, 0);
        assert_eq!(expected_module.1, address - image_base);
        assert_eq!(
            std::path::PathBuf::from(&expected_module.0),
            std::env::current_exe().unwrap()
        );
        let assert_context = |text: &str| {
            assert_eq!(number(text, "exception_address"), address as u64);
            assert_eq!(number(text, "thread_id"), tid as u64);
            assert_eq!(number(text, "module_offset"), expected_module.1 as u64);
            assert!(text.contains(&expected_module.0));
        };
        let result = record_native_dump(&description, "", "", &dmp, &txt, || {
            assert_context(&std::fs::read_to_string(&txt).unwrap());
            false
        });
        assert_eq!(
            result,
            DumpOutcome::Failed(dump_record::REASON_RETURNED_FALSE)
        );
        assert_context(&std::fs::read_to_string(&txt).unwrap());
    });
}

#[test]
fn unknown_module_metadata_survives_panicking_writer() {
    let dir = tempfile::tempdir().unwrap();
    let dmp = dir.path().join("probe.dmp");
    let txt = dir.path().join("probe.txt");
    supplied_context(1, 0x9876, |ctx| {
        let description = describe_ctx(ctx);
        let assert_context = |text: &str| {
            assert_eq!(number(text, "exception_address"), 1);
            assert_eq!(number(text, "thread_id"), 0x9876);
            assert_eq!(field(text, "module"), "unknown");
            assert_eq!(field(text, "module_offset"), "unknown");
        };
        let result = record_native_dump(&description, "", "", &dmp, &txt, || {
            assert_context(&std::fs::read_to_string(&txt).unwrap());
            panic!("injected writer failure");
        });
        assert_eq!(result, DumpOutcome::Failed(dump_record::REASON_PANICKED));
        assert_context(&std::fs::read_to_string(&txt).unwrap());
    });
}

#[test]
fn unmapped_exception_address_keeps_supplied_thread_and_unknown_module() {
    supplied_context(1, 42, |ctx| {
        let description = describe_ctx(ctx);
        assert_eq!(number(&description, "exception_address"), 1);
        assert_eq!(number(&description, "thread_id"), 42);
        assert_eq!(field(&description, "module"), "unknown");
        assert_eq!(field(&description, "module_offset"), "unknown");
        assert!(windows_context::module_at(1).is_none());
    });
}

#[test]
fn unreadable_or_missing_exception_records_are_unknown() {
    for pointer in [std::ptr::null(), 1usize as *const _] {
        let ctx = crash_context::CrashContext {
            exception_pointers: pointer,
            exception_code: ILLEGAL,
            process_id: std::process::id(),
            thread_id: 77,
        };
        let description = describe_ctx(&ctx);
        assert_eq!(field(&description, "exception_address"), "unknown");
        assert_eq!(number(&description, "thread_id"), 77);
        assert_eq!(field(&description, "module"), "unknown");
    }
    let pointers = crash_context::EXCEPTION_POINTERS {
        ExceptionRecord: std::ptr::null_mut(),
        ContextRecord: std::ptr::null_mut(),
    };
    let ctx = crash_context::CrashContext {
        exception_pointers: &pointers,
        exception_code: ILLEGAL,
        process_id: std::process::id(),
        thread_id: 88,
    };
    assert!(windows_context::exception_address(&ctx).is_none());
    supplied_context(0, 99, |ctx| {
        assert!(windows_context::exception_address(ctx).is_none());
    });
}

fn run_child(mode: &str, expected_exit: i32) -> std::path::PathBuf {
    let dir = tempfile::Builder::new()
        .prefix("codeterm-native-probe-")
        .tempdir()
        .unwrap()
        .keep();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "crash::native::tests::child_probe",
            "--nocapture",
        ])
        .env(CHILD_MODE, mode)
        .env(CHILD_DIR, &dir)
        .stdin(Stdio::null())
        .stdout(std::fs::File::create(dir.join("child.stdout.log")).unwrap())
        .stderr(std::fs::File::create(dir.join("child.stderr.log")).unwrap())
        .spawn()
        .unwrap();
    let status = child.wait_timeout(Duration::from_secs(30)).unwrap();
    let Some(status) = status else {
        child.kill().unwrap();
        child.wait().unwrap();
        panic!(
            "native child exceeded failure deadline; artifacts: {}",
            dir.display()
        );
    };
    eprintln!(
        "native probe mode={mode} exit={status} artifacts={}",
        dir.display()
    );
    assert_eq!(
        status.code(),
        Some(expected_exit),
        "artifacts: {}",
        dir.display()
    );
    dir
}

#[test]
fn interrupted_child_writer_preserves_original_metadata() {
    let dir = run_child("interrupted", 73);
    let text = std::fs::read_to_string(dir.join("probe.txt")).unwrap();
    let expected = std::fs::read_to_string(dir.join("expected.txt")).unwrap();
    assert!(text.contains(&expected));
    assert_eq!(number(&text, "thread_id"), 0x2468);
    assert_ne!(number(&text, "exception_address"), 0);
    assert_ne!(field(&text, "module"), "unknown");
    assert!(number(&text, "module_offset") < number(&text, "exception_address"));
    assert_eq!(std::fs::metadata(dir.join("probe.dmp")).unwrap().len(), 0);
    assert!(text.contains(&DumpOutcome::Attempted.render(&dir.join("probe.dmp"))));
}

#[test]
fn supplied_context_child_writes_readable_real_minidump() {
    let dir = run_child("supplied", 0);
    verify_dump(&dir);
}

#[test]
fn isolated_seh_child_writes_readable_real_minidump() {
    let dir = run_child("seh", 0);
    verify_dump(&dir);
}

fn probe_record(ctx: &crash_handler::CrashContext, dir: &Path, interrupted: bool) -> DumpOutcome {
    let description = describe_ctx(ctx);
    std::fs::write(dir.join("expected.txt"), &description).unwrap();
    let dmp = dir.join("probe.dmp");
    record_native_dump(&description, "", "", &dmp, &dir.join("probe.txt"), || {
        if interrupted {
            std::fs::File::create(&dmp).unwrap();
            std::process::exit(73);
        }
        write_minidump(ctx, &dmp)
    })
}

#[test]
fn child_probe() {
    let Ok(mode) = std::env::var(CHILD_MODE) else {
        return;
    };
    let dir = std::path::PathBuf::from(std::env::var_os(CHILD_DIR).unwrap());
    if mode == "seh" {
        // SAFETY: only this dedicated child installs a handler; the callback retains SEH pointers.
        let event = unsafe {
            make_crash_event(move |ctx| {
                let outcome = probe_record(ctx, &dir, false);
                std::process::exit(if matches!(outcome, DumpOutcome::Written { .. }) {
                    0
                } else {
                    75
                });
            })
        };
        let _handler = CrashHandler::attach(event).unwrap();
        // SAFETY: the synthetic exception is confined to this non-daemon child.
        unsafe { RaiseException(ILLEGAL as u32, 0, 0, std::ptr::null()) };
        panic!("SEH callback did not terminate child");
    }
    assert!(mode == "supplied" || mode == "interrupted");
    // SAFETY: syscall with no caller-owned buffers.
    let tid = if mode == "interrupted" {
        0x2468
    } else {
        unsafe { GetCurrentThreadId() }
    };
    supplied_context(marker as *const () as usize, tid, |ctx| {
        let outcome = probe_record(ctx, &dir, mode == "interrupted");
        assert!(
            matches!(outcome, DumpOutcome::Written { .. }),
            "{outcome:?}"
        );
    });
}

fn verify_dump(dir: &Path) {
    let data = std::fs::read(dir.join("probe.dmp")).unwrap();
    assert!(data.len() >= 32);
    assert_eq!(&data[..4], b"MDMP");
    let u32_at = |offset: usize| u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
    let count = u32_at(8) as usize;
    let directory = u32_at(12) as usize;
    assert!(directory >= 32 && directory <= data.len());
    assert!(count > 0 && count <= (data.len().saturating_sub(directory)) / 12);
    let mut streams = std::collections::HashMap::new();
    for index in 0..count {
        let entry = directory + index * 12;
        let size = u32_at(entry + 4) as usize;
        let rva = u32_at(entry + 8) as usize;
        assert!(rva <= data.len() && size <= data.len() - rva);
        streams.insert(u32_at(entry), (rva, size));
    }
    for stream in [3, 4, 7] {
        assert!(streams.get(&stream).is_some_and(|(_, size)| *size > 0));
    }
    let (exception, size) = streams[&6];
    assert!(size >= 168);
    let expected = std::fs::read_to_string(dir.join("expected.txt")).unwrap();
    assert_eq!(u32_at(exception) as u64, number(&expected, "thread_id"));
    assert_eq!(u32_at(exception + 8), ILLEGAL as u32);
    let address = u64::from_le_bytes(data[exception + 24..exception + 32].try_into().unwrap());
    assert_eq!(address, number(&expected, "exception_address"));
    let context_size = u32_at(exception + 160) as usize;
    let context_rva = u32_at(exception + 164) as usize;
    assert!(
        context_size > 0 && context_rva <= data.len() && context_size <= data.len() - context_rva
    );
    let text = std::fs::read_to_string(dir.join("probe.txt")).unwrap();
    assert!(text.contains(&expected));
    assert!(text.contains(
        &DumpOutcome::Written {
            bytes: data.len() as u64
        }
        .render(&dir.join("probe.dmp"))
    ));
}
