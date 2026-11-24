use crate::service::{MuxEmitCause, ServiceContext};

impl ServiceContext {
    pub fn update_tab_cwd(&self, tab_id: &str, cwd: String) {
        let changed = {
            let mut mux = self.mux.write();
            mux.get_tab_mut(tab_id).is_some_and(|tab| {
                let mapped = tab
                    .shell_profile
                    .as_ref()
                    .filter(|profile| profile.wsl_distro.is_some() && cwd.starts_with('/'))
                    .map(|profile| profile.host_cwd(&cwd))
                    .transpose();
                let Ok(mapped) = mapped else {
                    return false;
                };
                let cwd = codeterm_paths::strip_verbatim_prefix(&mapped.unwrap_or(cwd));
                if let Some(saved) = tab
                    .cwd
                    .as_deref()
                    .filter(|saved| same_observed_cwd(saved, &cwd))
                {
                    if tab.agent_booting && saved != cwd.as_str() {
                        crate::startup::boot_timing::mark(
                            tab_id,
                            tab.pty_id.as_deref(),
                            "launch_cwd_spelling_preserved",
                        );
                    }
                    return false;
                }
                tab.cwd = Some(cwd);
                true
            })
        };
        if changed {
            self.emit_state_changed(MuxEmitCause::Cwd);
        }
    }
}

fn same_observed_cwd(saved: &str, observed: &str) -> bool {
    if saved == observed {
        return true;
    }
    if !cfg!(windows)
        || saved.len() != observed.len()
        || !std::path::Path::new(saved).is_absolute()
        || !std::path::Path::new(observed).is_absolute()
    {
        return false;
    }
    let has_drive_letter =
        saved.as_bytes().get(1) == Some(&b':') && observed.as_bytes().get(1) == Some(&b':');
    saved
        .bytes()
        .zip(observed.bytes())
        .enumerate()
        .all(|(index, (saved, observed))| match (saved, observed) {
            (b'/', b'\\') | (b'\\', b'/') => true,
            _ if index == 0 && has_drive_letter => saved.eq_ignore_ascii_case(&observed),
            _ => saved == observed,
        })
}
