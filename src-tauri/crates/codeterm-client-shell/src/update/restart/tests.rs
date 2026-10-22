use super::*;

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
    let app = crate::app_runtime::builder().build(context).unwrap();
    let socket = std::path::PathBuf::from(format!("/tmp/{identifier}_si.sock"));
    let predecessor = UnixListener::bind(&socket).unwrap();
    assert!(UnixStream::connect(&socket).is_ok());
    release_single_instance(app.handle());
    assert!(UnixStream::connect(&socket).is_err());
    let replacement = UnixListener::bind(&socket).unwrap();
    assert!(UnixStream::connect(&socket).is_ok());
    drop(replacement);
    drop(predecessor);
    std::fs::remove_file(socket).unwrap();
}
