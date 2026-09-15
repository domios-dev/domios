//! The one owner of per-tab threads: notification sends, bridge sends, demand
//! opens, menu state, inbound routing and tab close all pass through here.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use serde::Serialize;

use super::compose;
use super::identity::{display_title, topic_name, SenderIdentity, ThreadTabs};
use super::registry::{ChatScope, ThreadBinding, ThreadRegistry};
use super::routing::{self, authorize_open, InboundRoute, ThreadCaller};
use super::transport::{topic_link, TopicFailure, TopicTransport};
use super::{ThreadError, SERVICE};
use crate::bridge::markdown::markdown_to_sanitized_html;
use crate::bridge::response::chunk_message;
use crate::bridge::telegram::TELEGRAM_MAX_LEN;
use crate::bridge::traits::InlineKeyboard;

/// Same window the main-chat `--append` edit uses.
const APPEND_WINDOW: Duration = Duration::from_secs(60);

#[derive(Clone)]
struct Attached {
    transport: Arc<dyn TopicTransport>,
    bot_id: i64,
}

struct AppendState {
    chat_id: i64,
    message_id: i64,
    header: Option<String>,
    markdown: String,
    sent_at: Instant,
}

/// One text a tab sends through the bridge.
pub struct TabText {
    pub tab_id: String,
    pub chat_id: i64,
    pub markdown: String,
    pub append: bool,
    pub buttons: Option<InlineKeyboard>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TopicsState {
    Available,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ThreadMenuState {
    pub bridge_active: bool,
    pub topics: TopicsState,
    pub bound: bool,
    pub link: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenedThread {
    pub tab_id: String,
    pub chat_id: i64,
    pub thread_id: i64,
    pub topic_name: String,
    pub link: Option<String>,
    pub created: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum CloseOutcome {
    Deleted {
        thread_id: i64,
    },
    Renamed {
        thread_id: i64,
        renamed: bool,
    },
    /// The bridge was off: the binding is retired without a Bot API call.
    Deferred {
        thread_id: i64,
    },
}

/// Where a tab's media lands and how it is attributed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaDestination {
    pub thread_id: Option<i64>,
    pub header: Option<String>,
}

enum Ensured {
    Bound {
        binding: ThreadBinding,
        created: bool,
    },
    Fallback {
        notice_due: bool,
        unavailable: bool,
        reason: Option<String>,
    },
}

pub struct ThreadService {
    registry: Mutex<ThreadRegistry>,
    attached: RwLock<Option<Attached>>,
    create_gate: tokio::sync::Mutex<()>,
    intro_gate: tokio::sync::Mutex<()>,
    appends: Mutex<HashMap<String, AppendState>>,
}

fn scope(bot_id: i64, chat_id: i64) -> ChatScope {
    ChatScope {
        service: SERVICE.to_string(),
        bot_id,
        chat_id,
    }
}

impl ThreadService {
    pub fn new(registry: ThreadRegistry) -> Self {
        Self {
            registry: Mutex::new(registry),
            attached: RwLock::new(None),
            create_gate: tokio::sync::Mutex::new(()),
            intro_gate: tokio::sync::Mutex::new(()),
            appends: Mutex::new(HashMap::new()),
        }
    }

    /// Bind the running bridge's transport. A new run probes topics again.
    pub fn attach(&self, transport: Arc<dyn TopicTransport>, bot_id: i64) {
        *self.attached.write() = Some(Attached { transport, bot_id });
        self.registry.lock().clear_topics_unavailable();
        self.appends.lock().clear();
    }

    pub fn detach(&self) {
        *self.attached.write() = None;
    }

    pub fn is_attached(&self) -> bool {
        self.attached.read().is_some()
    }

    fn attached(&self) -> Option<Attached> {
        self.attached.read().clone()
    }

    pub fn has_open_binding(&self, tab_id: &str) -> bool {
        !self
            .registry
            .lock()
            .open_bindings_for_tab(tab_id)
            .is_empty()
    }

    pub fn stored_json(&self) -> String {
        self.registry.lock().stored_json()
    }

    pub fn resolve_inbound(
        &self,
        tabs: &dyn ThreadTabs,
        chat_id: i64,
        thread_id: Option<i64>,
        reply_to_message_id: Option<i64>,
    ) -> InboundRoute {
        let bot_id = self.attached().map(|a| a.bot_id).unwrap_or_default();
        let lookup = {
            let registry = self.registry.lock();
            routing::lookup(
                &registry,
                &scope(bot_id, chat_id),
                thread_id,
                reply_to_message_id,
            )
        };
        routing::decide(lookup, &|tab_id| tabs.is_live_tab(tab_id))
    }

    /// Send a tab's text: into its topic with a headered main-chat pointer, or
    /// headered into the main chat when topics cannot carry it. Returns the
    /// message that carries the buttons.
    pub async fn route_text(&self, tabs: &dyn ThreadTabs, text: TabText) -> Option<i64> {
        let attached = self.attached()?;
        if text.append {
            if let Some(message_id) = self.append_to_recent(&attached, &text).await {
                return Some(message_id);
            }
        }
        let body = markdown_to_sanitized_html(&text.markdown);
        let identity = tabs.sender_identity(&text.tab_id);
        let header = identity.as_ref().map(SenderIdentity::header);
        let threaded = identity
            .as_ref()
            .filter(|_| !tabs.is_general_tab(&text.tab_id));

        let mut thread_id = None;
        if let Some(identity) = threaded {
            match self
                .ensure_thread(tabs, &attached, identity, text.chat_id, false)
                .await
            {
                Ensured::Bound { binding, .. } => thread_id = Some(binding.thread_id),
                Ensured::Fallback { notice_due, .. } => {
                    if notice_due {
                        self.send_notice(&attached, text.chat_id).await;
                    }
                }
            }
        }

        let html = match header.as_deref() {
            Some(header) => compose::with_header(header, &body),
            None => body,
        };
        let mut placed = self
            .send_chunks(
                &attached,
                text.chat_id,
                thread_id,
                &html,
                text.buttons.as_ref(),
            )
            .await;
        if let (Some(gone), Err(TopicFailure::ThreadGone)) = (thread_id, &placed) {
            self.registry
                .lock()
                .remove(&scope(attached.bot_id, text.chat_id), gone);
            thread_id = None;
            placed = self
                .send_chunks(&attached, text.chat_id, None, &html, text.buttons.as_ref())
                .await;
        }
        let placed = match placed {
            Ok(message_id) => message_id,
            Err(failure) => {
                log::warn!("[bridge-threads] tab send failed: {failure:?}");
                return None;
            }
        };
        self.record_origin(&attached, text.chat_id, placed, &text.tab_id);

        if let (Some(thread_id), Some(header), false) = (thread_id, header.as_deref(), text.append)
        {
            let link = topic_link(text.chat_id, thread_id);
            let pointer = compose::pointer(header, link.as_deref(), &text.markdown);
            if let Ok(pointer_id) = attached
                .transport
                .send_html(text.chat_id, None, &pointer, None)
                .await
            {
                self.record_origin(&attached, text.chat_id, pointer_id, &text.tab_id);
            }
        }

        self.appends.lock().insert(
            text.tab_id.clone(),
            AppendState {
                chat_id: text.chat_id,
                message_id: placed,
                header,
                markdown: text.markdown,
                sent_at: Instant::now(),
            },
        );
        Some(placed)
    }

    /// Resolve where a tab's media goes; the caller sends it, then calls
    /// [`Self::after_media`] with the placement.
    pub async fn media_destination(
        &self,
        tabs: &dyn ThreadTabs,
        tab_id: &str,
        chat_id: i64,
    ) -> MediaDestination {
        let Some(attached) = self.attached() else {
            return MediaDestination {
                thread_id: None,
                header: None,
            };
        };
        let Some(identity) = tabs.sender_identity(tab_id) else {
            return MediaDestination {
                thread_id: None,
                header: None,
            };
        };
        let header = Some(identity.header());
        if tabs.is_general_tab(tab_id) {
            return MediaDestination {
                thread_id: None,
                header,
            };
        }
        let thread_id = match self
            .ensure_thread(tabs, &attached, &identity, chat_id, false)
            .await
        {
            Ensured::Bound { binding, .. } => Some(binding.thread_id),
            Ensured::Fallback { notice_due, .. } => {
                if notice_due {
                    self.send_notice(&attached, chat_id).await;
                }
                None
            }
        };
        MediaDestination { thread_id, header }
    }

    pub async fn after_media(
        &self,
        tab_id: &str,
        chat_id: i64,
        destination: &MediaDestination,
        caption: &str,
    ) {
        let (Some(attached), Some(thread_id), Some(header)) = (
            self.attached(),
            destination.thread_id,
            destination.header.as_deref(),
        ) else {
            return;
        };
        let link = topic_link(chat_id, thread_id);
        let pointer = compose::pointer(header, link.as_deref(), caption);
        if let Ok(pointer_id) = attached
            .transport
            .send_html(chat_id, None, &pointer, None)
            .await
        {
            self.record_origin(&attached, chat_id, pointer_id, tab_id);
        }
    }

    /// Open or reuse `target_tab_id`'s topic. Authority is decided before any
    /// lookup or Bot API call, so a refused open leaves nothing behind.
    pub async fn open(
        &self,
        tabs: &dyn ThreadTabs,
        caller: ThreadCaller<'_>,
        target_tab_id: &str,
        chat_id: i64,
    ) -> Result<OpenedThread, ThreadError> {
        authorize_open(caller, target_tab_id)?;
        if !tabs.is_live_tab(target_tab_id) {
            return Err(ThreadError::UnknownTab(target_tab_id.to_string()));
        }
        if tabs.is_general_tab(target_tab_id) {
            return Err(ThreadError::GeneralUsesMainChat);
        }
        let attached = self.attached().ok_or(ThreadError::BridgeInactive)?;
        let identity = tabs
            .sender_identity(target_tab_id)
            .ok_or_else(|| ThreadError::UnknownTab(target_tab_id.to_string()))?;

        let (binding, created) = match self
            .ensure_thread(tabs, &attached, &identity, chat_id, true)
            .await
        {
            Ensured::Bound { binding, created } => (binding, created),
            Ensured::Fallback {
                notice_due,
                unavailable,
                reason,
            } => {
                if notice_due {
                    self.send_notice(&attached, chat_id).await;
                }
                return Err(if unavailable {
                    ThreadError::TopicsUnavailable
                } else {
                    ThreadError::CreationFailed(reason.unwrap_or_default())
                });
            }
        };

        self.post_intro_once(&attached, &identity, &binding).await;

        let link = topic_link(chat_id, binding.thread_id);
        Ok(OpenedThread {
            tab_id: binding.tab_id,
            chat_id,
            thread_id: binding.thread_id,
            topic_name: binding.topic_name,
            link,
            created,
        })
    }

    /// Retire every open thread of a closing tab. Delete first; on failure
    /// leave a closing note and a closed-prefix name. Never `closeForumTopic`.
    pub async fn close_tab(&self, tab_id: &str) -> Vec<CloseOutcome> {
        self.appends.lock().remove(tab_id);
        let bindings = self.registry.lock().open_bindings_for_tab(tab_id);
        let attached = self.attached();
        let mut outcomes = Vec::with_capacity(bindings.len());
        for binding in bindings {
            let chat_id = binding.scope.chat_id;
            let thread_id = binding.thread_id;
            let live = attached
                .as_ref()
                .filter(|attached| attached.bot_id == binding.scope.bot_id);
            let Some(attached) = live else {
                self.registry.lock().mark_closed(
                    &binding.scope,
                    thread_id,
                    compose::closed_topic_name(&binding.topic_name),
                );
                outcomes.push(CloseOutcome::Deferred { thread_id });
                continue;
            };
            match attached.transport.delete_topic(chat_id, thread_id).await {
                Ok(()) => {
                    self.registry.lock().remove(&binding.scope, thread_id);
                    outcomes.push(CloseOutcome::Deleted { thread_id });
                }
                Err(failure) => {
                    log::warn!("[bridge-threads] deleteForumTopic {thread_id} failed: {failure:?}");
                    let _ = attached
                        .transport
                        .send_html(chat_id, Some(thread_id), &compose::closing_note(), None)
                        .await;
                    let closed_name = compose::closed_topic_name(&binding.topic_name);
                    let renamed = attached
                        .transport
                        .rename_topic(chat_id, thread_id, &closed_name)
                        .await
                        .is_ok();
                    self.registry
                        .lock()
                        .mark_closed(&binding.scope, thread_id, closed_name);
                    outcomes.push(CloseOutcome::Renamed { thread_id, renamed });
                }
            }
        }
        outcomes
    }

    pub async fn menu_state(&self, tab_id: &str, chat_id: Option<i64>) -> ThreadMenuState {
        let (Some(attached), Some(chat_id)) = (self.attached(), chat_id) else {
            return ThreadMenuState {
                bridge_active: self.is_attached(),
                topics: TopicsState::Unknown,
                bound: self.has_open_binding(tab_id),
                link: None,
            };
        };
        let chat = scope(attached.bot_id, chat_id);
        let (binding, unavailable, any_bound) = {
            let registry = self.registry.lock();
            (
                registry.open_binding_for_tab(&chat, tab_id).cloned(),
                registry.topics_unavailable(&chat),
                registry.has_open_bindings(&chat),
            )
        };
        let topics = if unavailable {
            TopicsState::Unavailable
        } else if any_bound {
            TopicsState::Available
        } else {
            TopicsState::Unknown
        };
        let link = binding
            .as_ref()
            .and_then(|binding| topic_link(chat_id, binding.thread_id));
        ThreadMenuState {
            bridge_active: true,
            topics,
            bound: binding.is_some(),
            link,
        }
    }

    async fn ensure_thread(
        &self,
        tabs: &dyn ThreadTabs,
        attached: &Attached,
        identity: &SenderIdentity,
        chat_id: i64,
        probe: bool,
    ) -> Ensured {
        let chat = scope(attached.bot_id, chat_id);
        if let Some(binding) = self.bound(&chat, &identity.tab_id) {
            return Ensured::Bound {
                binding,
                created: false,
            };
        }
        if !probe && self.registry.lock().topics_unavailable(&chat) {
            return Ensured::Fallback {
                notice_due: false,
                unavailable: true,
                reason: None,
            };
        }
        let _gate = self.create_gate.lock().await;
        if let Some(binding) = self.bound(&chat, &identity.tab_id) {
            return Ensured::Bound {
                binding,
                created: false,
            };
        }
        let name = topic_name(&identity.title, &identity.tab_id, &tabs.live_tab_titles());
        match attached.transport.create_topic(chat_id, &name).await {
            Ok(thread_id) => {
                let binding = ThreadBinding {
                    scope: chat,
                    tab_id: identity.tab_id.clone(),
                    thread_id,
                    topic_name: name,
                    intro_posted: false,
                    closed: false,
                };
                self.registry.lock().bind(binding.clone());
                Ensured::Bound {
                    binding,
                    created: true,
                }
            }
            Err(TopicFailure::TopicsUnavailable) => {
                let notice_due = self.registry.lock().mark_topics_unavailable(&chat);
                Ensured::Fallback {
                    notice_due,
                    unavailable: true,
                    reason: None,
                }
            }
            Err(failure) => {
                log::warn!("[bridge-threads] createForumTopic failed: {failure:?}");
                Ensured::Fallback {
                    notice_due: false,
                    unavailable: false,
                    reason: Some(format!("{failure:?}")),
                }
            }
        }
    }

    /// Concurrent opens of one binding re-read the flag under the gate, so one intro lands.
    async fn post_intro_once(
        &self,
        attached: &Attached,
        identity: &SenderIdentity,
        binding: &ThreadBinding,
    ) {
        let _gate = self.intro_gate.lock().await;
        let posted = self
            .bound(&binding.scope, &binding.tab_id)
            .is_some_and(|current| current.intro_posted);
        if posted {
            return;
        }
        let chat_id = binding.scope.chat_id;
        let intro = compose::intro(
            &identity.header(),
            &display_title(&identity.title, &identity.tab_id),
        );
        if let Ok(message_id) = attached
            .transport
            .send_html(chat_id, Some(binding.thread_id), &intro, None)
            .await
        {
            self.record_origin(attached, chat_id, message_id, &binding.tab_id);
            self.registry
                .lock()
                .mark_intro_posted(&binding.scope, binding.thread_id);
        }
    }

    fn bound(&self, chat: &ChatScope, tab_id: &str) -> Option<ThreadBinding> {
        self.registry
            .lock()
            .open_binding_for_tab(chat, tab_id)
            .cloned()
    }

    async fn append_to_recent(&self, attached: &Attached, text: &TabText) -> Option<i64> {
        let (chat_id, message_id, header, combined) = {
            let mut appends = self.appends.lock();
            let state = appends.get_mut(&text.tab_id)?;
            if state.chat_id != text.chat_id || state.sent_at.elapsed() >= APPEND_WINDOW {
                return None;
            }
            state.markdown = format!("{}\n\n{}", state.markdown, text.markdown);
            state.sent_at = Instant::now();
            (
                state.chat_id,
                state.message_id,
                state.header.clone(),
                state.markdown.clone(),
            )
        };
        let body = markdown_to_sanitized_html(&combined);
        let html = match header.as_deref() {
            Some(header) => compose::with_header(header, &body),
            None => body,
        };
        if html.len() > TELEGRAM_MAX_LEN {
            return None;
        }
        attached
            .transport
            .edit_html(chat_id, message_id, &html)
            .await
            .then_some(message_id)
    }

    async fn send_chunks(
        &self,
        attached: &Attached,
        chat_id: i64,
        thread_id: Option<i64>,
        html: &str,
        buttons: Option<&InlineKeyboard>,
    ) -> Result<i64, TopicFailure> {
        let chunks = chunk_message(html, TELEGRAM_MAX_LEN);
        let last = chunks.len().saturating_sub(1);
        let mut placed = Err(TopicFailure::Rejected("empty message".to_string()));
        for (index, chunk) in chunks.iter().enumerate() {
            let keyboard = if index == last { buttons } else { None };
            placed = attached
                .transport
                .send_html(chat_id, thread_id, chunk, keyboard)
                .await;
            if placed.is_err() {
                return placed;
            }
        }
        placed
    }

    async fn send_notice(&self, attached: &Attached, chat_id: i64) {
        let _ = attached
            .transport
            .send_html(chat_id, None, &compose::notice(), None)
            .await;
    }

    fn record_origin(&self, attached: &Attached, chat_id: i64, message_id: i64, tab_id: &str) {
        self.registry
            .lock()
            .record_origin(&scope(attached.bot_id, chat_id), message_id, tab_id);
    }
}
