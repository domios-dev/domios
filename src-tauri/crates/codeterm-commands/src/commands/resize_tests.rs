use std::sync::Arc;

use codeterm_service_core::lock_hold::TimedMutex;
use codeterm_service_core::pty::{manager::PtyManager, screen_state::ScreenState};

#[test]
fn client_resize_updates_pty_and_screen_geometry() {
    let pty = Arc::new(TimedMutex::new("pty", PtyManager::new()));
    let screens = Arc::new(TimedMutex::new("screen_state", ScreenState::new()));
    let (id, session) = pty
        .lock()
        .spawn(None, None, 120, 40, Default::default())
        .unwrap();
    codeterm_service_core::service::pty_resize_resize(&pty, &screens, &id, None).unwrap();
    crate::commands::resize_pty_core(&pty, &screens, id.clone(), 152, 58).unwrap();
    assert_eq!(pty.lock().size(&id), Some((152, 58)));
    let applied = session
        .pair
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .master
        .get_size()
        .unwrap();
    assert_eq!((applied.cols, applied.rows), (152, 58));
    assert_eq!(screens.lock().get_screen_rows(&id).unwrap().len(), 58);
    screens.lock().process(&id, b"\x1b[58;152HX");
    assert_eq!(
        screens.lock().get_screen_rows(&id).unwrap()[57],
        format!("{}X", " ".repeat(151))
    );

    screens.lock().resize(&id, 120, 40);
    codeterm_service_core::service::pty_resize_resize(&pty, &screens, &id, None).unwrap();
    assert_eq!(screens.lock().get_screen_rows(&id).unwrap().len(), 58);
    assert_eq!(pty.lock().size(&id), Some((152, 58)));
    assert!(
        codeterm_service_core::service::pty_resize_resize(&pty, &screens, &id, Some((0, 24)))
            .is_err()
    );
    assert_eq!(pty.lock().size(&id), Some((152, 58)));
    assert_eq!(screens.lock().get_screen_rows(&id).unwrap().len(), 58);
    pty.lock().kill(&id).unwrap();
}

#[test]
fn missing_pty_resize_does_not_create_a_screen() {
    let pty = Arc::new(TimedMutex::new("pty", PtyManager::new()));
    let screens = Arc::new(TimedMutex::new("screen_state", ScreenState::new()));
    assert!(codeterm_service_core::service::pty_resize_resize(
        &pty,
        &screens,
        "missing",
        Some((80, 24))
    )
    .is_err());
    assert!(screens.lock().get_screen_text("missing").is_none());
}
