pub(super) fn ownership_for_spawning_peer(
    mut ownership: Option<codeterm_types::pty::RemoteOwnership>,
    peer_id: &str,
    peer_name: &str,
) -> codeterm_types::pty::RemoteOwnership {
    let owner = ownership.get_or_insert(codeterm_types::pty::RemoteOwnership {
        controller_title: peer_name.into(),
        controller_tab_id: None,
        peer_id: String::new(),
        peer_name: String::new(),
        team: None,
    });
    owner.peer_id = peer_id.into();
    owner.peer_name = peer_name.into();
    owner.clone()
}
