use super::super::restored_leaf_backing;
use super::{chatbackend_leaf, snapshot_with};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{SurfaceBacking, TabBacking};
use crate::session::store::SplitNodeSnapshot;

fn board() -> SurfaceBacking {
    SurfaceBacking {
        plugin_id: "dash".into(),
        surface_id: "board".into(),
        instance_id: "inst-1".into(),
        owner_label: true,
        resume_url: None,
        resume_profile: None,
    }
}

fn surface_leaf(tab_id: &str) -> SplitNodeSnapshot {
    let mut node = chatbackend_leaf(tab_id, "unused", "unused");
    if let SplitNodeSnapshot::Leaf {
        chatbackend_plugin_id,
        chatbackend_session_id,
        surface,
        chat_view,
        title,
        ..
    } = &mut node
    {
        *chatbackend_plugin_id = None;
        *chatbackend_session_id = None;
        *surface = Some(board());
        *chat_view = None;
        *title = Some("Board".into());
    }
    node
}

#[test]
fn surface_backing_restores_and_round_trips_through_snapshot() {
    let state = MultiplexerState::from_session_snapshot(&snapshot_with(surface_leaf("s1")));
    let tab = state.tabs.get("s1").expect("surface tab restored");
    assert_eq!(tab.backing, TabBacking::Surface(board()));
    assert!(tab.pty_id.is_none());
    assert!(tab.agent_type.is_none());
    assert!(
        !tab.scrollback_seed_pending,
        "a surface has no terminal to seed"
    );

    let round = state.to_session_snapshot();
    match &round.workspaces[0].tabs[0].split_tree {
        SplitNodeSnapshot::Leaf {
            surface,
            chatbackend_plugin_id,
            ..
        } => {
            assert_eq!(surface.as_ref(), Some(&board()));
            assert!(chatbackend_plugin_id.is_none());
        }
        SplitNodeSnapshot::Split { .. } => panic!("expected a leaf"),
    }
}

#[test]
fn restored_leaf_backing_prefers_surface_then_chat_then_pty() {
    assert_eq!(
        restored_leaf_backing(None, None, Some(&board())),
        TabBacking::Surface(board())
    );
    assert_eq!(
        restored_leaf_backing(Some("lmstudio"), Some("sess"), None),
        TabBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "sess".into(),
        }
    );
    assert_eq!(
        restored_leaf_backing(Some("lmstudio"), None, None),
        TabBacking::Pty
    );
}

fn browser_with_resume() -> SurfaceBacking {
    SurfaceBacking {
        plugin_id: "browser".into(),
        surface_id: "browser".into(),
        instance_id: "inst-2".into(),
        owner_label: true,
        resume_url: Some("https://a.dev/inbox#/thread/7".into()),
        resume_profile: Some("work".into()),
    }
}

#[test]
fn surface_resume_point_round_trips_through_a_serialized_snapshot() {
    let mut leaf = surface_leaf("s1");
    if let SplitNodeSnapshot::Leaf { surface, .. } = &mut leaf {
        *surface = Some(browser_with_resume());
    }
    let json = serde_json::to_string(&snapshot_with(leaf)).unwrap();
    let persisted: crate::session::store::SessionSnapshot = serde_json::from_str(&json).unwrap();
    let state = MultiplexerState::from_session_snapshot(&persisted);
    assert_eq!(
        state.tabs.get("s1").unwrap().backing,
        TabBacking::Surface(browser_with_resume())
    );
}

#[test]
fn surface_snapshot_without_a_resume_point_restores_and_writes_none() {
    let mut json = serde_json::to_value(snapshot_with(surface_leaf("s1"))).unwrap();
    let surface = json["workspaces"][0]["tabs"][0]["split_tree"]["surface"]
        .as_object_mut()
        .unwrap();
    assert!(!surface.contains_key("resume_url"));
    assert!(!surface.contains_key("resume_profile"));
    surface.remove("owner_label");
    let legacy: crate::session::store::SessionSnapshot = serde_json::from_value(json).unwrap();
    let state = MultiplexerState::from_session_snapshot(&legacy);
    let restored = state.tabs.get("s1").unwrap().surface().unwrap().clone();
    assert_eq!(restored.resume_url, None);
    assert_eq!(restored.resume_profile, None);
    assert_eq!(restored.instance_id, "inst-1");
}
