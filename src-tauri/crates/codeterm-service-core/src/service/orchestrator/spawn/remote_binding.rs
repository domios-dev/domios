pub(crate) fn remote_task_binding_matches_authority(
    remote: Option<&super::super::types::RemoteTaskBinding>,
    authority: Option<&super::super::types::TaskLaunchAuthority>,
) -> bool {
    let identity = authority.map(
        |authority| crate::service::startup_lease::StartupTaskIdentity {
            plan_id: authority.plan_id.clone(),
            task_id: authority.task_id.clone(),
            attempt_id: authority.attempt_id.clone(),
        },
    );
    remote_task_binding_matches(remote, identity.as_ref())
}

pub(super) fn remote_task_binding_matches(
    remote: Option<&super::super::types::RemoteTaskBinding>,
    identity: Option<&crate::service::startup_lease::StartupTaskIdentity>,
) -> bool {
    remote.zip(identity).is_some_and(|(remote, identity)| {
        !remote.origin_peer_id.trim().is_empty()
            && remote.plan_id == identity.plan_id
            && remote.task_id == identity.task_id
            && remote.attempt_id == identity.attempt_id
    })
}
