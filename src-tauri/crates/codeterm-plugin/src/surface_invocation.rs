use serde::Serialize;
use std::{cell::RefCell, collections::BTreeMap};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceInvocationContext {
    pub plugin: String,
    pub registration: String,
    pub surface: String,
    pub instance: String,
    pub tab: String,
    pub owner: String,
    pub actor: String,
    pub workspace: String,
    pub team: Option<String>,
}

struct Ticket {
    context: SurfaceInvocationContext,
    active: bool,
}
struct Invocation {
    id: String,
    plugin: String,
    registration: Option<String>,
    tickets: BTreeMap<String, Ticket>,
    active: Vec<String>,
    resume: Option<(String, String, bool)>,
}
thread_local! { static CURRENT: RefCell<Option<Invocation>> = const { RefCell::new(None) }; }

pub(crate) struct RootScope(Option<Invocation>);
impl RootScope {
    pub(crate) fn enter(plugin: &str, registration: Option<String>) -> Self {
        Self(CURRENT.with(|slot| {
            slot.replace(Some(Invocation {
                id: uuid::Uuid::new_v4().simple().to_string(),
                plugin: plugin.into(),
                registration,
                tickets: BTreeMap::new(),
                active: Vec::new(),
                resume: None,
            }))
        }))
    }
}

pub(crate) fn root_identity() -> String {
    CURRENT.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|root| root.id.clone())
            .unwrap_or_default()
    })
}

pub(crate) fn take_mode(key: &str, job: &str, owner: &str) -> String {
    CURRENT.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(root) = slot.as_mut().filter(|root| root.id == owner) else {
            return String::new();
        };
        if !root
            .resume
            .as_ref()
            .is_some_and(|(expected_key, expected_job, _)| {
                expected_key == key && expected_job == job
            })
        {
            return String::new();
        }
        match root.resume.take() {
            Some((_, _, true)) => "discard".into(),
            Some(_) => "resume".into(),
            None => String::new(),
        }
    })
}

pub(crate) struct ResumeScope {
    previous: Option<(String, String, bool)>,
    root: String,
}
impl ResumeScope {
    pub(crate) fn enter(key: &str, job: &str, discard: bool) -> Self {
        let previous = CURRENT.with(|slot| {
            slot.borrow_mut()
                .as_mut()
                .and_then(|root| root.resume.replace((key.into(), job.into(), discard)))
        });
        Self {
            previous,
            root: root_identity(),
        }
    }
}
impl Drop for ResumeScope {
    fn drop(&mut self) {
        CURRENT.with(|slot| {
            if let Some(root) = slot
                .borrow_mut()
                .as_mut()
                .filter(|root| root.id == self.root)
            {
                root.resume = self.previous.take();
            }
        });
    }
}
impl Drop for RootScope {
    fn drop(&mut self) {
        CURRENT.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}

pub fn issue(context: SurfaceInvocationContext) -> Result<String, String> {
    CURRENT.with(|slot| {
        let mut slot = slot.borrow_mut();
        let invocation = slot
            .as_mut()
            .filter(|invocation| {
                invocation.plugin == context.plugin
                    && invocation.registration.as_deref() == Some(context.registration.as_str())
            })
            .ok_or("surface invocation root unavailable")?;
        if invocation.tickets.len() >= 64 {
            return Err("surface invocation limit reached".into());
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        invocation.tickets.insert(
            id.clone(),
            Ticket {
                context,
                active: false,
            },
        );
        Ok(id)
    })
}

pub fn current(plugin: &str) -> Option<SurfaceInvocationContext> {
    CURRENT.with(|slot| {
        let slot = slot.borrow();
        let invocation = slot
            .as_ref()
            .filter(|invocation| invocation.plugin == plugin)?;
        invocation
            .tickets
            .get(invocation.active.last()?)
            .map(|ticket| ticket.context.clone())
    })
}

pub(crate) fn begin(id: &str) -> bool {
    CURRENT.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(invocation) = slot.as_mut() else {
            return false;
        };
        if invocation.active.len() >= 16 {
            return false;
        }
        let Some(ticket) = invocation
            .tickets
            .get_mut(id)
            .filter(|ticket| !ticket.active)
        else {
            return false;
        };
        ticket.active = true;
        invocation.active.push(id.into());
        true
    })
}

pub(crate) fn end(id: &str, suspended: bool) -> bool {
    CURRENT.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(invocation) = slot.as_mut() else {
            return false;
        };
        if invocation.active.last().map(String::as_str) != Some(id) {
            return false;
        }
        invocation.active.pop();
        if suspended {
            if let Some(ticket) = invocation.tickets.get_mut(id) {
                ticket.active = false;
            }
        } else {
            invocation.tickets.remove(id);
        }
        true
    })
}

#[cfg(test)]
mod tests;
