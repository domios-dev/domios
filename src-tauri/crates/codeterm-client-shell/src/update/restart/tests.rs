use super::*;

#[test]
fn cleanup_is_dispatched_to_the_main_thread_before_completion() {
    use std::sync::mpsc::{channel, TryRecvError};

    let main_thread = std::thread::current().id();
    let (dispatch, pending) = channel();
    let (cleaned, observed) = channel();
    let completion = std::thread::spawn(move || {
        schedule_cleanup_with(
            move |cleanup| {
                dispatch.send(cleanup).unwrap();
                Ok(())
            },
            move || cleaned.send(std::thread::current().id()).unwrap(),
        )
        .unwrap()
    })
    .join()
    .unwrap();

    assert_eq!(observed.try_recv(), Err(TryRecvError::Empty));
    assert_eq!(completion.try_recv(), Err(TryRecvError::Empty));
    pending.try_recv().unwrap()();
    assert_eq!(observed.try_recv().unwrap(), main_thread);
    assert_eq!(completion.try_recv(), Ok(()));
    assert_eq!(completion.try_recv(), Err(TryRecvError::Disconnected));
}

#[test]
fn cleanup_is_not_run_when_main_thread_dispatch_fails() {
    let (cleaned, observed) = std::sync::mpsc::channel();
    let result = schedule_cleanup_with(
        |_| Err("event loop closed".into()),
        move || cleaned.send(()).unwrap(),
    );

    assert!(result.unwrap_err().contains("event loop closed"));
    assert_eq!(
        observed.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    );
}

#[test]
fn relaunch_executes_the_selected_binary_directly() {
    let executable = std::path::Path::new("/Applications/Domios.app/Contents/MacOS/Domios");
    let command = relaunch_command(executable);
    assert_eq!(command.get_program(), executable);
    assert_eq!(command.get_args().count(), 0);
}

#[cfg(target_os = "macos")]
#[test]
fn replacement_can_own_the_single_instance_socket_while_predecessor_is_alive() {
    use std::os::unix::net::{UnixListener, UnixStream};

    const CHILD: &str = "DOMIOS_SINGLE_INSTANCE_RESTART_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "update::restart::tests::replacement_can_own_the_single_instance_socket_while_predecessor_is_alive",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "isolated single-instance fixture failed: {}\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed;"));
        return;
    }

    let (ready, listening) = tokio::sync::oneshot::channel();
    let ready = std::sync::Mutex::new(Some(ready));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .on_thread_park(move || {
            if let Some(ready) = ready.lock().unwrap().take() {
                let _ = ready.send(());
            }
        })
        .build()
        .unwrap();
    tauri::async_runtime::set(runtime.handle().clone());

    let mut context = tauri::test::mock_context(tauri::test::noop_assets());
    let root = tempfile::tempdir().unwrap();
    let identifier = format!(
        "relaunch_{}",
        root.path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .replace('.', "_")
    );
    context.config_mut().identifier = identifier.clone();
    let app = crate::app_runtime::builder()
        .plugin(tauri_plugin_single_instance::init(|_, _, _| {}))
        .build(context)
        .unwrap();
    let socket = std::path::PathBuf::from(format!("/tmp/{identifier}_si.sock"));
    // The isolated executor parks only after the plugin listener yields at accept.
    runtime.block_on(async {
        tokio::time::timeout(std::time::Duration::from_secs(10), listening)
            .await
            .expect("single-instance listener did not become idle")
            .unwrap();
    });
    assert!(UnixStream::connect(&socket).is_ok());
    release_single_instance(app.handle());
    assert!(UnixStream::connect(&socket).is_err());
    let replacement = UnixListener::bind(&socket).unwrap();
    assert!(UnixStream::connect(&socket).is_ok());
    app.cleanup_before_exit();
    assert!(UnixStream::connect(&socket).is_ok());
    drop(replacement);
    std::fs::remove_file(socket).unwrap();
}
