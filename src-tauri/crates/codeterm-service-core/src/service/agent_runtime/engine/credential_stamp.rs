use crate::service::ServiceContext;
use codeterm_plugin::provider_account::InvocationContext;
use std::{
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::service::agent_runtime) struct CredentialStamp(
    Vec<(String, Option<(u64, Option<u128>)>)>,
);

pub(super) fn capture(
    svc: &ServiceContext,
    provider: &str,
    invocation: Option<&InvocationContext>,
) -> Option<CredentialStamp> {
    let capability = svc
        .agent_registry
        .with_provider(provider, |adapter| adapter.account_capability())
        .flatten()?;
    if capability.credential_files.is_empty() {
        return None;
    }
    let home = match invocation {
        Some(context) => context
            .environment_entries()
            .find(|(name, _)| *name == capability.home_environment)
            .and_then(|(_, value)| value.map(PathBuf::from))?,
        None => PathBuf::from(
            crate::service::provider_accounts::usual_home_for_login(&capability).ok()?,
        ),
    };
    Some(stamp_in(&home, &capability.credential_files))
}

pub(in crate::service::agent_runtime) async fn changed(
    svc: &ServiceContext,
    session: &super::Session,
) -> bool {
    let Some(previous) = session.credential_stamp.lock().clone() else {
        return false;
    };
    let owner = svc.clone();
    let provider = session.provider.clone();
    let invocation = session.invocation.lock().clone();
    let current = codeterm_plugin::js_runtime::spawn_blocking(move || {
        capture(&owner, &provider, invocation.as_deref())
    })
    .await
    .ok()
    .flatten();
    current.is_some_and(|current| current != previous)
}

fn stamp_in(home: &Path, files: &[String]) -> CredentialStamp {
    let entries = files
        .iter()
        .map(|name| {
            let state = std::fs::metadata(home.join(name)).ok().map(|meta| {
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map(|duration| duration.as_nanos());
                (meta.len(), modified)
            });
            (name.clone(), state)
        })
        .collect();
    CredentialStamp(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_stamp_detects_creation_update_and_removal_without_reading_contents() {
        let dir = tempfile::tempdir().unwrap();
        let files = vec!["auth.json".to_string()];
        let missing = stamp_in(dir.path(), &files);
        assert_eq!(missing, stamp_in(dir.path(), &files));

        let auth = dir.path().join("auth.json");
        std::fs::write(&auth, b"opaque-a").unwrap();
        let created = stamp_in(dir.path(), &files);
        assert_ne!(missing, created);
        assert_eq!(created, stamp_in(dir.path(), &files));

        std::fs::write(&auth, b"opaque-longer-credential").unwrap();
        let updated = stamp_in(dir.path(), &files);
        assert_ne!(created, updated);

        std::fs::remove_file(&auth).unwrap();
        assert_eq!(stamp_in(dir.path(), &files), missing);
    }
}
