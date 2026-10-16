use parking_lot::{Condvar, Mutex};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Priority {
    Focused,
    Visible,
    Background,
}

impl Priority {
    pub fn from_visibility(
        focused: bool,
        active_workspace: bool,
        shown: bool,
        hidden: bool,
    ) -> Self {
        if focused {
            Self::Focused
        } else if active_workspace && shown && !hidden {
            Self::Visible
        } else {
            Self::Background
        }
    }
}

pub fn priority(mux: &crate::multiplexer::state::MultiplexerState, tab_id: &str) -> Priority {
    let tab = mux.get_tab(tab_id);
    Priority::from_visibility(
        mux.get_focused_tab_id() == Some(tab_id),
        mux.get_active_workspace()
            .is_some_and(|workspace| workspace.tabs.iter().any(|id| id == tab_id)),
        tab.is_some_and(|tab| tab.show_in_tabbar),
        tab.is_none_or(|tab| tab.hidden),
    )
}

#[derive(Default)]
pub struct Changes {
    revision: Mutex<u64>,
    changed: Condvar,
}

impl Changes {
    pub fn revision(&self) -> u64 {
        *self.revision.lock()
    }

    pub fn notify(&self) {
        *self.revision.lock() += 1;
        self.changed.notify_all();
    }

    pub fn wait(&self, observed: u64) {
        let mut revision = self.revision.lock();
        while *revision == observed {
            self.changed.wait(&mut revision);
        }
    }
}

pub fn changes() -> &'static Changes {
    static CHANGES: OnceLock<Changes> = OnceLock::new();
    CHANGES.get_or_init(Changes::default)
}

pub fn notify_changed() {
    changes().notify();
}
