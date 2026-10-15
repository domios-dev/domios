use std::time::Duration;

use tokio::sync::watch;

use super::runtime::SurfaceResume;
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{SurfaceBacking, TabBacking};
use crate::service::ServiceContext;
use crate::session::store::SaveIntent;

/// Rapid navigations (redirect chains, SPA hops) collapse into one desk write.
const RESUME_COALESCE: Duration = Duration::from_millis(750);

/// What a restored tab hands its first runtime launch; `None` when nothing was remembered.
pub(crate) fn backing_resume(backing: &SurfaceBacking) -> Option<SurfaceResume> {
    let resume = SurfaceResume {
        url: backing.resume_url.clone(),
        profile: backing.resume_profile.clone(),
    };
    (!resume.is_empty()).then_some(resume)
}

/// Records `resume` on the tab's surface backing; `None` once the tab is gone.
pub(crate) fn store_surface_resume(
    mux: &mut MultiplexerState,
    tab_id: &str,
    resume: &SurfaceResume,
) -> Option<bool> {
    let TabBacking::Surface(backing) = &mut mux.get_tab_mut(tab_id)?.backing else {
        return None;
    };
    if backing.resume_url == resume.url && backing.resume_profile == resume.profile {
        return Some(false);
    }
    backing.resume_url = resume.url.clone();
    backing.resume_profile = resume.profile.clone();
    Some(true)
}

/// Blocking: stores `resume` and submits the desk snapshot when it changed.
pub(crate) fn write_surface_resume(
    svc: &ServiceContext,
    mux: &MuxState,
    tab_id: &str,
    resume: &SurfaceResume,
) -> Option<bool> {
    let changed = store_surface_resume(&mut mux.write(), tab_id, resume)?;
    if changed {
        if let Err(error) = crate::service::session_persist::submit_session_snapshot_with_intent(
            svc,
            mux,
            SaveIntent::PreserveDesk,
        ) {
            log::warn!("[surface] resume point not submitted: {error}");
        }
    }
    Some(changed)
}

impl ServiceContext {
    /// Follows a runtime's resume point into the desk until the runtime or its tab is gone.
    pub(super) fn follow_surface_resume(
        &self,
        tab_id: String,
        updates: watch::Receiver<SurfaceResume>,
    ) {
        let mux = self.mux.clone();
        let svc = self.clone();
        crate::async_runtime::spawn(follow_resume(tab_id, updates, mux, svc));
    }
}

async fn follow_resume(
    tab_id: String,
    mut updates: watch::Receiver<SurfaceResume>,
    mux: MuxState,
    svc: ServiceContext,
) {
    while updates.changed().await.is_ok() {
        tokio::time::sleep(RESUME_COALESCE).await;
        let resume = updates.borrow_and_update().clone();
        let (mux, svc, tab) = (mux.clone(), svc.clone(), tab_id.clone());
        let written =
            tokio::task::spawn_blocking(move || write_surface_resume(&svc, &mux, &tab, &resume))
                .await;
        if !matches!(written, Ok(Some(_))) {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mux_with_surface() -> (MultiplexerState, String) {
        let (mut mux, _) = MultiplexerState::new();
        let workspace_id = mux.list_workspaces()[0].id.clone();
        let tab_id = mux.create_tab(&workspace_id).unwrap();
        mux.get_tab_mut(&tab_id).unwrap().backing = TabBacking::Surface(SurfaceBacking {
            plugin_id: "browser".into(),
            surface_id: "browser".into(),
            instance_id: "i".into(),
            owner_label: true,
            resume_url: None,
            resume_profile: None,
        });
        (mux, tab_id)
    }

    #[test]
    fn surface_resume_is_stored_on_the_backing_and_reports_changes() {
        let (mut mux, tab_id) = mux_with_surface();
        let resume = SurfaceResume {
            url: Some("https://a.dev/".into()),
            profile: Some("work".into()),
        };
        assert_eq!(store_surface_resume(&mut mux, &tab_id, &resume), Some(true));
        assert_eq!(
            store_surface_resume(&mut mux, &tab_id, &resume),
            Some(false)
        );
        let backing = mux.get_tab(&tab_id).unwrap().surface().unwrap().clone();
        assert_eq!(backing.resume_url.as_deref(), Some("https://a.dev/"));
        assert_eq!(backing_resume(&backing), Some(resume));
        assert_eq!(
            store_surface_resume(&mut mux, "missing", &SurfaceResume::default()),
            None
        );
    }

    #[test]
    fn surface_resume_never_lands_on_a_non_surface_tab() {
        let (mut mux, owner_id) = MultiplexerState::new();
        let resume = SurfaceResume {
            url: Some("https://a.dev/".into()),
            profile: None,
        };
        assert_eq!(store_surface_resume(&mut mux, &owner_id, &resume), None);
        assert_eq!(mux.get_tab(&owner_id).unwrap().backing, TabBacking::Pty);
    }

    #[test]
    fn surface_backing_without_resume_hands_nothing_to_the_runtime() {
        let (mux, tab_id) = mux_with_surface();
        assert_eq!(
            backing_resume(mux.get_tab(&tab_id).unwrap().surface().unwrap()),
            None
        );
    }
}
