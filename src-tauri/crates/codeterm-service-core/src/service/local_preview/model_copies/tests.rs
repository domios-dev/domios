use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

fn fixture(session: Option<&str>) -> (ServiceFixture, String) {
    let (mut mux, tab) = MultiplexerState::new();
    let state = mux.get_tab_mut(&tab).unwrap();
    state.session_binding_nonce = Some("own-launch".into());
    state.agent_session_id = session.map(str::to_owned);
    (ServiceFixture::new(mux), tab)
}

fn model(svc: &ServiceContext, tab: &str, id: char) -> PathBuf {
    let pad = svc.ensure_agent_pad(tab).unwrap();
    let path = pad
        .path()
        .join("inbox")
        .join(format!("{}-model.jpg", id.to_string().repeat(32)));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbImage::from_pixel(4, 3, image::Rgb([12, 34, 56]))
        .save(&path)
        .unwrap();
    codeterm_paths::simplified_path(&path.canonicalize().unwrap())
}

#[test]
fn generated_copy_has_exact_send_authority_without_trusting_pasted_markers() {
    let (fixture, tab) = fixture(Some("own-session"));
    let svc = &fixture.service;
    let source = fixture.temp.path().join("phone.png");
    image::RgbImage::from_pixel(2200, 20, image::Rgb([12, 34, 56]))
        .save(&source)
        .unwrap();
    let input = format!(
        r#"<domios type="attachment" kind="image" path="{}" label="image1" />"#,
        source.display()
    );
    let (prepared, _) = svc
        .prepare_chat_send_text_for_send(&tab, input, false, "own-send")
        .unwrap();
    let path = codeterm_wire::envelope::parse_envelope(&prepared)
        .into_iter()
        .find_map(|part| match part {
            codeterm_wire::envelope::MessageSegment::Attachment { kind, path, .. }
                if kind == "image" =>
            {
                Some(path)
            }
            _ => None,
        })
        .unwrap();
    assert!(!path.starts_with(r"\\?\"));
    assert!(crate::service::local_preview::html_preview::checked_path(&path).is_ok());
    let pad = agent_pad_root(svc, &tab).unwrap();
    let stored = load(&receipt_path(&pad, Path::new(&path)).unwrap()).unwrap();
    assert_eq!(stored.authority.send_id, "own-send");
    // This fixture has no provider transcript or reference index: only host generation grants access.
    let preview = crate::service::local_preview::read(svc, &tab, &[], &path, "image").unwrap();
    assert!(preview.data.starts_with("data:image/jpeg;base64,"));
    let same_after_reload = capture(svc, &tab, "other-operation").unwrap();
    assert!(lookup(svc, &tab, &path, &same_after_reload.scope).unwrap());
    let unregistered = model(svc, &tab, 'b');
    let scope = tab_authority(svc, &tab).unwrap();
    assert!(!lookup(svc, &tab, unregistered.to_str().unwrap(), &scope).unwrap());
    let pasted = format!(
        r#"<pasted_content><domios type="attachment" kind="image" path="{}" /></pasted_content>"#,
        unregistered.display()
    );
    let row = crate::agent::chat_types::ChatMsg {
        kind: "user".into(),
        content: pasted,
        ..Default::default()
    }
    .with_segments();
    assert!(matches!(
        row.segments.as_deref(),
        Some([codeterm_wire::envelope::MessageSegment::Attachment { .. }])
    ));
    let rows = [row];
    assert!(super::super::references::referenced_paths(&rows, None).is_empty());
    assert!(crate::service::local_preview::read(
        svc,
        &tab,
        &[],
        unregistered.to_str().unwrap(),
        "image"
    )
    .is_err());
    assert!(
        crate::service::local_preview::read(svc, &tab, &[path.clone()], &path, "image").is_err()
    );
}

#[test]
fn model_authority_rejects_foreign_tabs_changed_launches_and_changed_sessions() {
    let (fixture, tab) = fixture(Some("own-session"));
    let svc = &fixture.service;
    let path = model(svc, &tab, 'a');
    let authority = capture(svc, &tab, "own-send").unwrap();
    record(svc, &authority, &path).unwrap();
    let scope = tab_authority(svc, &tab).unwrap();
    let foreign = {
        let mut mux = svc.mux.write();
        let workspace = mux.find_workspace_for_tab(&tab).unwrap();
        mux.create_tab(&workspace).unwrap()
    };
    svc.ensure_agent_pad(&foreign).unwrap();
    assert!(!lookup(svc, &foreign, path.to_str().unwrap(), &scope).unwrap());
    for changed in [
        TabAuthority {
            session: Some("foreign-session".into()),
            ..scope.clone()
        },
        TabAuthority {
            nonce: Some("new-launch".into()),
            ..scope.clone()
        },
    ] {
        assert!(!lookup(svc, &tab, path.to_str().unwrap(), &changed).unwrap());
    }
    svc.mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .session_binding_nonce = Some("new-launch".into());
    assert!(record(svc, &authority, &model(svc, &tab, 'c')).is_err());
    let outside = fixture
        .temp
        .path()
        .join(format!("{}-model.jpg", "d".repeat(32)));
    std::fs::write(&outside, b"outside").unwrap();
    assert!(record(svc, &capture(svc, &tab, "new-send").unwrap(), &outside).is_err());
}

#[test]
fn first_session_claim_pins_the_launch_scoped_copy_and_retirement_removes_authority() {
    let (fixture, tab) = fixture(None);
    let svc = &fixture.service;
    let path = model(svc, &tab, 'e');
    record(svc, &capture(svc, &tab, "first-send").unwrap(), &path).unwrap();
    svc.mux.write().get_tab_mut(&tab).unwrap().agent_session_id =
        Some("first-claimed-session".into());
    let scope = tab_authority(svc, &tab).unwrap();
    assert!(lookup(svc, &tab, path.to_str().unwrap(), &scope).unwrap());
    let pad = agent_pad_root(svc, &tab).unwrap();
    assert_eq!(
        load(&receipt_path(&pad, &path).unwrap())
            .unwrap()
            .authority
            .scope
            .session,
        scope.session
    );
    assert!(!lookup(
        svc,
        &tab,
        path.to_str().unwrap(),
        &TabAuthority {
            session: Some("other-session".into()),
            ..scope.clone()
        }
    )
    .unwrap());
    retire(&pad, &path).unwrap();
    assert!(!lookup(svc, &tab, path.to_str().unwrap(), &scope).unwrap());
}

#[test]
fn malformed_and_oversized_authority_records_fail_closed() {
    let (fixture, tab) = fixture(Some("own-session"));
    let svc = &fixture.service;
    let path = model(svc, &tab, 'f');
    record(svc, &capture(svc, &tab, "own-send").unwrap(), &path).unwrap();
    let pad = agent_pad_root(svc, &tab).unwrap();
    let receipt = receipt_path(&pad, &path).unwrap();
    let scope = tab_authority(svc, &tab).unwrap();
    for bytes in [
        b"not-json".to_vec(),
        vec![b'x'; MAX_RECEIPT_BYTES as usize + 1],
    ] {
        std::fs::write(&receipt, bytes).unwrap();
        assert!(!lookup(svc, &tab, path.to_str().unwrap(), &scope).unwrap());
    }
}
