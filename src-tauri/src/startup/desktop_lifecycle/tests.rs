use super::*;

fn fixture() -> codeterm_host_runtime::app_runtime::App {
    let mut context = crate::app_context();
    context.config_mut().build.dev_url = None;
    context.config_mut().app.windows = vec![tauri::utils::config::WindowConfig {
        create: false,
        ..Default::default()
    }];
    let app = tauri::test::mock_builder().build(context).unwrap();
    app.manage(DesktopLifecycle::default());
    super::super::frontend_shell::initialize(app.handle()).unwrap();
    app
}

fn authenticated_window(
    app: &AppHandle,
) -> Result<codeterm_host_runtime::app_runtime::WebviewWindow, Box<dyn std::error::Error>> {
    use crate::host_ports::ServerApiAttachmentSlot;
    use codeterm_types::server_bootstrap::{
        ProcessProvenance, ServerIdentity, ServerReady, API_VERSION, BOOTSTRAP_PROTOCOL,
    };
    let slot = ServerApiAttachmentSlot::default();
    slot.install(
        ServerReady {
            protocol: BOOTSTRAP_PROTOCOL,
            api_version: API_VERSION,
            build: "fixture-build".into(),
            instance_id: "fixture-instance".into(),
            identity: ServerIdentity {
                data: "fixture-data".into(),
                config: "fixture-config".into(),
            },
            epoch: "fixture-epoch".into(),
            process: ProcessProvenance {
                pid: 1,
                created: "fixture-creation".into(),
                executable: "fixture-server".into(),
            },
            api_address: "127.0.0.1:17686".parse().unwrap(),
            socket_address: "fixture-socket".into(),
            bootstrap_address: "fixture-bootstrap".into(),
            resources: "fixture-resources".into(),
        },
        "fixture-token".into(),
    )
    .unwrap();
    app.manage(slot);
    super::super::frontend_shell::create_window(app)
}

#[test]
fn activation_wires_authenticated_creation_then_raises_the_same_main_window() {
    let app = fixture();
    show_or_create_with(app.handle(), authenticated_window).unwrap();
    assert_eq!(app.webview_windows().len(), 1);
    show_or_create_with(app.handle(), |_| panic!("duplicate frontend creator")).unwrap();
    assert_eq!(app.webview_windows().len(), 1);
    assert_eq!(*app.state::<DesktopLifecycle>().0.lock(), Frontend::Open);
}

#[test]
fn activation_surfaces_creation_failure_and_accepts_a_later_activation() {
    let app = fixture();
    let error =
        show_or_create_with(app.handle(), |_| Err("fixture creation failure".into())).unwrap_err();
    assert_eq!(error.to_string(), "fixture creation failure");
    assert!(app.webview_windows().is_empty());
    show_or_create_with(app.handle(), authenticated_window).unwrap();
    assert_eq!(app.webview_windows().len(), 1);
}

#[test]
fn activation_during_native_creation_cannot_create_another_window() {
    let app = fixture();
    show_or_create_with(app.handle(), |handle| {
        show_or_create_with(handle, |_| panic!("concurrent frontend creator"))?;
        authenticated_window(handle)
    })
    .unwrap();
    assert_eq!(app.webview_windows().len(), 1);
}

#[test]
fn close_invalidates_only_the_frontend_and_waits_for_native_destruction() {
    let app = fixture();
    show_or_create_with(app.handle(), authenticated_window).unwrap();
    let window = app.get_webview_window("main").unwrap();
    assert!(close_requested(&window.as_ref().window()));
    show_or_create_with(app.handle(), |_| {
        panic!("creation before native destruction")
    })
    .unwrap();
    assert_eq!(
        *app.state::<DesktopLifecycle>().0.lock(),
        Frontend::Closing { reopen: true }
    );
    assert!(retain_daemon(app.handle(), None));
    assert!(!retain_daemon(app.handle(), Some(0)));
    assert_eq!(
        *app.state::<DesktopLifecycle>().0.lock(),
        Frontend::ShuttingDown
    );
}
