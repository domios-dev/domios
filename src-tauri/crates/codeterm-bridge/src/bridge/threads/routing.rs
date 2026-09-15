//! Who an inbound message belongs to, and who may open whose thread.

use super::registry::{ChatScope, ThreadBinding, ThreadRegistry};
use super::ThreadError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundOrigin {
    /// The thread's tab closed and deleting its topic failed.
    ClosedThread { topic_name: String },
    /// A thread no binding knows, including one already deleted.
    UnboundThread,
    /// The thread or replied-to message belongs to a tab that is gone.
    ClosedTab { tab_id: String },
}

impl InboundOrigin {
    pub fn marker_value(&self) -> &'static str {
        match self {
            Self::ClosedThread { .. } => "closed_thread",
            Self::UnboundThread => "unbound_thread",
            Self::ClosedTab { .. } => "closed_tab",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundRoute {
    Tab {
        tab_id: String,
        thread_id: Option<i64>,
    },
    General {
        thread_id: Option<i64>,
        origin: Option<InboundOrigin>,
    },
}

/// Registry facts for one inbound message, owned so no lock is held while tabs are asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundLookup {
    Thread {
        thread_id: i64,
        binding: Option<ThreadBinding>,
    },
    ReplyToTab {
        tab_id: String,
    },
    Main,
}

pub fn lookup(
    registry: &ThreadRegistry,
    scope: &ChatScope,
    thread_id: Option<i64>,
    reply_to_message_id: Option<i64>,
) -> InboundLookup {
    if let Some(thread_id) = thread_id {
        return InboundLookup::Thread {
            thread_id,
            binding: registry.binding_for_thread(scope, thread_id).cloned(),
        };
    }
    reply_to_message_id
        .and_then(|message_id| registry.origin_tab(scope, message_id))
        .map(|tab_id| InboundLookup::ReplyToTab {
            tab_id: tab_id.to_string(),
        })
        .unwrap_or(InboundLookup::Main)
}

pub fn decide(lookup: InboundLookup, is_live: &dyn Fn(&str) -> bool) -> InboundRoute {
    match lookup {
        InboundLookup::Thread {
            thread_id,
            binding: Some(binding),
        } if binding.closed => InboundRoute::General {
            thread_id: Some(thread_id),
            origin: Some(InboundOrigin::ClosedThread {
                topic_name: binding.topic_name,
            }),
        },
        InboundLookup::Thread {
            thread_id,
            binding: Some(binding),
        } if is_live(&binding.tab_id) => InboundRoute::Tab {
            tab_id: binding.tab_id,
            thread_id: Some(thread_id),
        },
        InboundLookup::Thread {
            thread_id,
            binding: Some(binding),
        } => InboundRoute::General {
            thread_id: Some(thread_id),
            origin: Some(InboundOrigin::ClosedTab {
                tab_id: binding.tab_id,
            }),
        },
        InboundLookup::Thread {
            thread_id,
            binding: None,
        } => InboundRoute::General {
            thread_id: Some(thread_id),
            origin: Some(InboundOrigin::UnboundThread),
        },
        InboundLookup::ReplyToTab { tab_id } if is_live(&tab_id) => InboundRoute::Tab {
            tab_id,
            thread_id: None,
        },
        InboundLookup::ReplyToTab { tab_id } => InboundRoute::General {
            thread_id: None,
            origin: Some(InboundOrigin::ClosedTab { tab_id }),
        },
        InboundLookup::Main => InboundRoute::General {
            thread_id: None,
            origin: None,
        },
    }
}

pub fn resolve_inbound(
    registry: &ThreadRegistry,
    scope: &ChatScope,
    thread_id: Option<i64>,
    reply_to_message_id: Option<i64>,
    is_live: &dyn Fn(&str) -> bool,
) -> InboundRoute {
    decide(
        lookup(registry, scope, thread_id, reply_to_message_id),
        is_live,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadCaller<'a> {
    /// A tab proven by its transport; the General Agent may name another tab.
    Tab { tab_id: &'a str, is_general: bool },
    /// The user acting through a tab's own menu.
    Owner,
}

pub fn authorize_open(caller: ThreadCaller<'_>, target_tab_id: &str) -> Result<(), ThreadError> {
    match caller {
        ThreadCaller::Owner | ThreadCaller::Tab {
            is_general: true, ..
        } => Ok(()),
        ThreadCaller::Tab { tab_id, .. } if tab_id == target_tab_id => Ok(()),
        ThreadCaller::Tab { tab_id, .. } => Err(ThreadError::ForeignTarget {
            caller: tab_id.to_string(),
            target: target_tab_id.to_string(),
        }),
    }
}
