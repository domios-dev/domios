use super::*;
use codeterm_local_preview::artifact_directive::serialize_file_card;

fn home() -> PathBuf {
    PathBuf::from(if cfg!(windows) {
        "C:/fixture"
    } else {
        "/fixture"
    })
}

fn raw(path: &Path) -> String {
    path.to_str().unwrap().to_owned()
}

#[test]
fn admission_pins_exact_own_pad_files_and_canvas_items() {
    let home = home();
    let pad = home.join("pads/sender");
    let board = pad.join("board.canvas.json");
    let child = pad.join("items/report.md");
    let unreferenced = pad.join("other.md");
    let foreign = home.join("pads/another/file.md");
    let arbitrary = home.join("work.md");
    let linked = pad.join("linked.md");
    let escaped = pad.join("../sender/out.md");
    let standalone = pad.join("image.png");
    let child_paths = [&child, &foreign, &arbitrary, &linked, &escaped];
    let canvas = format!(
        r#"{{"format":"domios.canvas/1","items":{}}}"#,
        serde_json::json!(child_paths.map(|path| serde_json::json!({"kind":"document","src":path})))
    );
    let card = |kind, path: &Path| serialize_file_card(kind, &[raw(path)], None).unwrap();
    let body = [
        card("canvas", &board),
        card("image", &standalone),
        card("document", &foreign),
        card("document", &arbitrary),
        card("document", &linked),
        card("document", &escaped),
        format!("```text\n{}\n```", card("document", &unreferenced)),
    ]
    .join("\n");
    let cards = scan_artifacts(&body, true).cards;
    let grants = admit(
        "sender",
        "recipient",
        &pad,
        &cards,
        0,
        |src, read_canvas| {
            let resolved = PathBuf::from(src);
            if resolved == linked {
                return Err("linked component".into());
            }
            Ok((resolved, read_canvas.then(|| canvas.as_bytes().to_vec())))
        },
    )
    .unwrap();
    assert_eq!(grants.len(), 2);
    assert_eq!(grants[0].file, board);
    assert_eq!(grants[0].items, [child.clone()].into());
    assert_eq!(grants[1].file, standalone);
    let grants = Grants(grants);
    assert!(grants.find("recipient", &child, false).is_some());
    assert!(grants.find("recipient", &child, true).is_none());
    assert!(grants.find("recipient", &unreferenced, false).is_none());
    assert!(grants.find("other-tab", &child, false).is_none());
}

#[test]
fn either_tab_close_revokes_every_share_involving_that_tab() {
    let pad = home().join("pads/sender");
    let file = pad.join("shared.md");
    let grant = |sender: &str, recipient: &str| Grant {
        sender: sender.into(),
        recipient: recipient.into(),
        pad: pad.clone(),
        file: file.clone(),
        items: Default::default(),
    };
    for closed in ["sender", "recipient"] {
        let mut grants = Grants::default();
        grants.insert(grant("sender", "recipient"));
        grants.insert(grant("independent-sender", "independent-recipient"));
        assert!(grants.revoke(closed));
        assert!(grants.find("recipient", &file, false).is_none());
        assert!(grants.find("independent-recipient", &file, false).is_some());
        assert!(!grants.revoke(closed));
    }
}

#[test]
fn pad_share_store_restores_and_persists_revocation() {
    let temp = tempfile::tempdir().unwrap();
    let state = temp.path().join("shares.json");
    let pad = home().join("pads/sender");
    let file = pad.join("shared.md");
    let mut grants = Grants::default();
    grants.insert(Grant {
        sender: "sender".into(),
        recipient: "recipient".into(),
        pad,
        file: file.clone(),
        items: Default::default(),
    });
    save(&state, &grants).unwrap();
    let mut restored = load(&state).unwrap();
    assert!(restored.find("recipient", &file, false).is_some());
    assert!(restored.revoke("sender"));
    save(&state, &restored).unwrap();
    assert!(load(&state)
        .unwrap()
        .find("recipient", &file, false)
        .is_none());
}
