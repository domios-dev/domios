use super::*;
use crate::provider::AgentProvider;

#[test]
fn find_session_under_returns_the_enumerated_record_for_exactly_that_id() {
    let tmp = tempfile::tempdir().unwrap();
    let mut env = codeterm_util::test_env::EnvGuard::lock();
    env.set(
        crate::session_evidence::EVIDENCE_DIR_ENV,
        tmp.path().join("store"),
    );
    let root = tmp.path().join("projects");
    let owned = "cccc3333-find-4ccc-8ccc-cccccccccccc";
    let sibling = "dddd4444-find-4ddd-8ddd-dddddddddddd";
    let other_dir = root.join("-other-project");
    let owned_dir = root.join("-owned-project");
    std::fs::create_dir_all(&other_dir).unwrap();
    std::fs::create_dir_all(&owned_dir).unwrap();
    std::fs::write(
        other_dir.join(format!("{sibling}.jsonl")),
        "{\"cwd\":\"/other\"}\n",
    )
    .unwrap();
    std::fs::write(
        owned_dir.join(format!("{owned}.jsonl")),
        "{\"cwd\":\"/owned\"}\n",
    )
    .unwrap();
    std::fs::write(
        owned_dir.join("sessions-index.json"),
        format!(r#"{{"entries":[{{"sessionId":"{owned}","title":"Owned","summary":"S"}}]}}"#),
    )
    .unwrap();

    let found = find_session_under(&root, owned).expect("owned session resolves by id");
    let enumerated = enumerate_sessions_under(&root)
        .into_iter()
        .find(|s| s.session_id == owned)
        .expect("fixture session is enumerated");
    assert_eq!(
        serde_json::to_value(&found).unwrap(),
        serde_json::to_value(&enumerated).unwrap()
    );
    assert_eq!(found.project_path.as_deref(), Some("/owned"));
    assert_eq!(found.title.as_deref(), Some("Owned"));

    assert!(find_session_under(&root, "eeee5555-none-4eee-8eee-eeeeeeeeeeee").is_none());
    assert!(find_session_under(
        &root,
        "../-owned-project/cccc3333-find-4ccc-8ccc-cccccccccccc"
    )
    .is_none());
    assert!(find_session_under(&root, "").is_none());
}

#[test]
fn test_encode_cwd() {
    assert_eq!(encode_cwd("/home/user/project"), "-home-user-project");
    assert_eq!(encode_cwd("C:\\Users\\test"), "C--Users-test");
}

#[test]
fn encode_cwd_matches_real_claude_project_dirs() {
    let cases = [
        ("/Users/user", "-Users-user"),
        // /Users/user/.domios-dev/general : the '/' before '.domios' AND the '.'
        // each become a dash -> the `--codeterm` double-dash. Real dir on disk.
        (
            "/Users/user/.domios-dev/general",
            "-Users-user--domios-dev-general",
        ),
        (
            "/Users/user/Developer/wk-cb-p1",
            "-Users-user-Developer-wk-cb-p1",
        ),
        // Windows native path: drive colon + backslashes all collapse to dashes.
        ("C:\\Users\\dev\\proj", "C--Users-dev-proj"),
        // encoder is separator-agnostic, so a normalized cwd binds the same dir.
        ("C:/Users/dev/proj", "C--Users-dev-proj"),
    ];
    for (cwd, want) in cases {
        assert_eq!(encode_cwd(cwd), want, "encode_cwd({cwd:?})");
    }
}

#[test]
fn detect_session_id_refuses_to_guess_when_ambiguous_in_shared_cwd() {
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().to_string_lossy().to_string();
    let s1 = "aaaa1111-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    let s2 = "bbbb2222-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
    let Some(p1) = session_file_path(&cwd, s1) else {
        return;
    };
    let Some(dir) = p1.parent().map(|d| d.to_path_buf()) else {
        return;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let p2 = dir.join(format!("{s2}.jsonl"));
    if std::fs::write(&p1, "{}\n").is_err() || std::fs::write(&p2, "{}\n").is_err() {
        return;
    }

    // Two recent sibling sessions, nothing excluded → ambiguous → no guess.
    assert_eq!(
        detect_session_id(&cwd, &[], Some(Duration::from_secs(300))),
        None,
        "must refuse to guess when 2+ sibling sessions are viable candidates",
    );

    // Excluding the sibling disambiguates → the unique candidate binds.
    let got = detect_session_id(&cwd, &[s2.to_string()], Some(Duration::from_secs(300)));
    assert_eq!(
        got.as_deref(),
        Some(s1),
        "with the sibling excluded there is exactly one candidate → bind it",
    );

    let _ = std::fs::remove_file(&p1);
    let _ = std::fs::remove_file(&p2);
}

#[test]
fn detect_active_session_never_grabs_a_sibling_session_in_same_cwd() {
    let p = super::super::ClaudeProvider::new();
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().to_string_lossy().to_string();
    let my_sid = "mine-0000-0000-0000-000000000000"; // tab's own id (no file yet)
    let sibling_sid = "sibl-1111-1111-1111-111111111111";
    let Some(sib_path) = p.session_file_path(&cwd, sibling_sid) else {
        return;
    };
    let Some(dir) = sib_path.parent().map(|d| d.to_path_buf()) else {
        return;
    };
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    // Materialize ONLY the sibling's session in this shared cwd.
    if std::fs::write(&sib_path, "{\"cwd\":\"x\"}\n").is_err() {
        return;
    }
    assert_eq!(
        p.detect_active_session(&cwd, my_sid),
        None,
        "must never roll a tab onto a sibling's session (no newest-for-cwd)",
    );
    let _ = std::fs::remove_file(&sib_path);
}

#[test]
fn test_verbatim_cwd_encodes_like_plain_cwd() {
    // SAME claude project dir as the plain cwd — otherwise resume never matches.
    let plain = encode_cwd(r"C:\Users\dev\.domios-dev\general");
    let verbatim = encode_cwd(&codeterm_paths::strip_verbatim_prefix(
        r"\\?\C:\Users\dev\.domios-dev\general",
    ));
    assert_eq!(plain, verbatim);
    assert_eq!(plain, "C--Users-dev--domios-dev-general");
}

#[test]
fn test_sessions_dir_real_cwd_has_no_verbatim_dashes() {
    let cwd = std::env::temp_dir().to_string_lossy().to_string();
    let dir = sessions_dir(&cwd).unwrap();
    let leaf = dir.file_name().unwrap().to_string_lossy().to_string();
    assert!(
        !leaf.starts_with("----"),
        "verbatim prefix leaked into encoded project dir: {leaf}"
    );
}

#[test]
fn test_session_file_path() {
    let path = session_file_path("/home/user/project", "abc-def-123-456").unwrap();
    let path_str = path.to_string_lossy();
    assert!(path_str.contains("abc-def-123-456.jsonl"));
    assert!(path_str.contains(".claude"));
    assert!(path_str.contains("projects"));
    assert!(path_str.contains("-home-user-project"));
}

#[test]
fn session_file_path_prefers_stored_transcript_path_over_reconstruction() {
    let tmp = tempfile::tempdir().unwrap();
    let mut env = codeterm_util::test_env::EnvGuard::lock();
    env.set(
        crate::session_evidence::EVIDENCE_DIR_ENV,
        tmp.path().join("store"),
    );

    // Stored transcript whose filename differs from the session id; the
    // reconstructed <sid>.jsonl is never created — reads must not need it.
    let sid = "aaaa1111-read-pref-4aaa-8aaa-aaaaaaaaaaaa";
    let stored = tmp.path().join("vendor-drifted-uuid.jsonl");
    std::fs::write(&stored, "{\"cwd\":\"x\"}\n").unwrap();
    assert!(crate::session_evidence::record(
        sid,
        stored.to_str().unwrap()
    ));

    let cwd = tmp.path().to_string_lossy().to_string();
    let resolved = session_file_path(&cwd, sid).expect("stored path resolves");
    assert_eq!(resolved, stored, "stored path wins over reconstruction");
    let reconstructed = sessions_dir(&cwd).unwrap().join(format!("{sid}.jsonl"));
    assert!(
        !reconstructed.exists(),
        "fixture layout: the reconstructed path must not exist"
    );
    assert!(
        crate::provider_account::read_to_string(&resolved).is_ok(),
        "the stored file itself is readable through the preferred path"
    );
}

#[test]
fn session_file_path_without_transcript_path_evidence_reconstructs_as_today() {
    let tmp = tempfile::tempdir().unwrap();
    let mut env = codeterm_util::test_env::EnvGuard::lock();
    env.set(
        crate::session_evidence::EVIDENCE_DIR_ENV,
        tmp.path().join("empty-store"),
    );

    let cwd = tmp.path().to_string_lossy().to_string();
    let sid = "bbbb2222-no-evidence-4bbb-8bbb-bbbbbbbbbbbb";
    let resolved = session_file_path(&cwd, sid).expect("reconstruction resolves");
    let reconstructed = sessions_dir(&cwd).unwrap().join(format!("{sid}.jsonl"));
    assert_eq!(
        resolved, reconstructed,
        "absence of evidence keeps today's reconstruction byte-for-byte"
    );
}

#[test]
fn test_session_exists_default_via_file_path() {
    let p = super::super::ClaudeProvider::new();
    let tmp = tempfile::tempdir().unwrap();
    let cwd = tmp.path().to_string_lossy().to_string();

    // find it. Skips the test if HOME isn't writable.
    let real_path = match p.session_file_path(&cwd, "session-real") {
        Some(p) => p,
        None => return,
    };
    if let Some(parent) = real_path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    if std::fs::write(&real_path, "{}\n").is_err() {
        return;
    }

    assert!(
        p.session_exists(&cwd, "session-real") == Some(true),
        "existing JSONL should be detected"
    );
    assert!(
        p.session_exists(&cwd, "session-bogus-uuid-xxx") != Some(true),
        "missing JSONL must report false"
    );

    let _ = std::fs::remove_file(&real_path);
}
