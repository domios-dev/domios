use serde::{Deserialize, Serialize};

use super::tab_menu::TabMenuItem;
use crate::provider::ContextOwner;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatPreprocessorCap {
    pub enabled: bool,
    /// Declared pipeline slot (`match`). `None` ⇒ the text slot, the only slot a
    /// preprocessor may grab today (the SDK fixes `match: "text"`).
    pub match_slot: Option<String>,
}

impl ChatPreprocessorCap {
    pub fn enabled(&self) -> bool {
        self.enabled
    }
}

impl<'de> Deserialize<'de> for ChatPreprocessorCap {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Flag(bool),
            Obj {
                #[serde(default, rename = "match")]
                match_slot: Option<String>,
            },
        }
        Ok(match Raw::deserialize(d)? {
            Raw::Flag(enabled) => Self {
                enabled,
                match_slot: None,
            },
            Raw::Obj { match_slot } => Self {
                enabled: true,
                match_slot,
            },
        })
    }
}

impl Serialize for ChatPreprocessorCap {
    /// Symmetric with `Deserialize`: a slot ⇒ object, otherwise the bare bool —
    /// so a disabled cap round-trips to `false`, never silently re-enabling.
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match &self.match_slot {
            Some(slot) if self.enabled => {
                use serde::ser::SerializeMap;
                let mut m = s.serialize_map(Some(1))?;
                m.serialize_entry("match", slot)?;
                m.end()
            }
            _ => s.serialize_bool(self.enabled),
        }
    }
}

/// `secretBackend` capability declaration. Authors write either a bare bool
/// (`true` — the plugin is the secret store but never restores a session on its
/// own) or an object carrying capability flags. Both forms mean "enabled".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecretBackendCap {
    pub enabled: bool,
    /// The backend restores a usable session without interactive input, from
    /// credentials it persisted itself. Declaring it makes the host OFFER
    /// auto-unlock; it never authorizes a vault operation, which still has to
    /// pass the vault's own authentication.
    pub auto_unlock: bool,
}

impl SecretBackendCap {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn auto_unlock(&self) -> bool {
        self.enabled && self.auto_unlock
    }
}

impl<'de> Deserialize<'de> for SecretBackendCap {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Flag(bool),
            Obj {
                #[serde(default, rename = "autoUnlock")]
                auto_unlock: bool,
            },
        }
        Ok(match Raw::deserialize(d)? {
            Raw::Flag(enabled) => Self {
                enabled,
                auto_unlock: false,
            },
            Raw::Obj { auto_unlock } => Self {
                enabled: true,
                auto_unlock,
            },
        })
    }
}

impl Serialize for SecretBackendCap {
    /// Symmetric with `Deserialize`: an undeclared auto-unlock round-trips to
    /// the bare bool, so a re-serialized manifest can never silently grant it.
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if self.enabled && self.auto_unlock {
            use serde::ser::SerializeMap;
            let mut m = s.serialize_map(Some(1))?;
            m.serialize_entry("autoUnlock", &true)?;
            m.end()
        } else {
            s.serialize_bool(self.enabled)
        }
    }
}

/// `chatBackend` capability declaration. Authors write either a bare bool
/// (`true` — the backend is registered but NOT user-spawnable, e.g. a
/// host-/agent-driven NPC tab) or an object carrying capability flags. Both
/// forms mean "enabled"; the object also carries whether a user may spawn it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatBackendCap {
    pub enabled: bool,
    /// Whether a user may open this backend as a tab from the new-session
    /// picker. Bare `chatBackend: true` ⇒ false (backend-only); the object form
    /// `{ spawnable: true }` opts in.
    pub spawnable: bool,
    pub watcher: bool,
    /// The backend accepts host-composed instructions in the `openSession`
    /// systemPrompt context.
    pub session_config: bool,
    /// Who composes the context this backend sends. Absent ⇒ `Plugin`, today's
    /// exact behaviour; an unrecognized string fails the parse.
    pub context_owner: ContextOwner,
}

impl ChatBackendCap {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn spawnable(&self) -> bool {
        self.spawnable
    }

    pub fn watcher(&self) -> bool {
        self.watcher
    }

    pub fn session_config(&self) -> bool {
        self.session_config
    }

    pub fn context_owner(&self) -> ContextOwner {
        self.context_owner
    }
}

impl<'de> Deserialize<'de> for ChatBackendCap {
    fn deserialize<D>(d: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Flag(bool),
            Obj {
                #[serde(default)]
                spawnable: bool,
                #[serde(default)]
                watcher: bool,
                #[serde(default, rename = "sessionConfig")]
                session_config: bool,
                #[serde(default, rename = "contextOwner")]
                context_owner: Option<String>,
            },
        }
        Ok(match Raw::deserialize(d)? {
            Raw::Flag(enabled) => Self {
                enabled,
                spawnable: false,
                watcher: false,
                session_config: false,
                context_owner: ContextOwner::default(),
            },
            Raw::Obj {
                spawnable,
                watcher,
                session_config,
                context_owner,
            } => {
                let context_owner = match context_owner {
                    None => ContextOwner::default(),
                    Some(declared) => match ContextOwner::parse(&declared) {
                        Some(owner) => owner,
                        // A typo resolving to `plugin` would reinstate an unverifiable promise.
                        None => {
                            return Err(serde::de::Error::custom(format!(
                                "unknown chatBackend contextOwner '{declared}': expected \"plugin\" or \"host\""
                            )))
                        }
                    },
                };
                Self {
                    enabled: true,
                    spawnable,
                    watcher,
                    session_config,
                    context_owner,
                }
            }
        })
    }
}

impl Serialize for ChatBackendCap {
    /// Symmetric with `Deserialize`: object flags stay objects, while disabled
    /// and bare backend-only caps stay bools.
    fn serialize<S>(&self, s: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let host_context = !self.context_owner.is_plugin();
        let declares = self.spawnable || self.watcher || self.session_config || host_context;
        if self.enabled && declares {
            use serde::ser::SerializeMap;
            let len = (if self.spawnable { 1 } else { 0 })
                + (if self.watcher { 1 } else { 0 })
                + (if self.session_config { 1 } else { 0 })
                + (if host_context { 1 } else { 0 });
            let mut m = s.serialize_map(Some(len))?;
            if self.spawnable {
                m.serialize_entry("spawnable", &true)?;
            }
            if self.watcher {
                m.serialize_entry("watcher", &true)?;
            }
            if self.session_config {
                m.serialize_entry("sessionConfig", &true)?;
            }
            if host_context {
                m.serialize_entry("contextOwner", &self.context_owner)?;
            }
            m.end()
        } else {
            s.serialize_bool(self.enabled)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WebhookReceiverKind {
    Pebble,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WebhookReceiverCap {
    pub kind: WebhookReceiverKind,
}

/// The core capability traits a plugin can implement. Drives which adapters the
/// loader builds (`JsProvider` / `JsSecretBackend` / `JsTranscriber` /
/// `JsGlanceProvider` / `JsBubbleProvider`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Capabilities {
    #[serde(default)]
    pub agent_provider: bool,
    /// Bool (the plugin is the store) or `{ autoUnlock }` object (the store
    /// also declares it can restore its own session headlessly).
    #[serde(default)]
    pub secret_backend: SecretBackendCap,
    #[serde(default)]
    pub transcriber: bool,
    /// Renders a read-only "glance" popover (ViewNode tree) when pinned.
    #[serde(default)]
    pub glance_view: bool,
    /// Contributes a status pill into the tab-footer bubble row.
    #[serde(default)]
    pub status_bubble: bool,
    /// Ships a full HTML/JS/CSS UI (`ui/index.html`), rendered in a sandboxed
    /// iframe (VS Code/Figma webview model). Talks to the host only through the
    /// permission-gated `ct.invoke` postMessage bridge → the plugin's QuickJS
    /// exports. Behind a loud install grant.
    #[serde(default)]
    pub view: bool,
    /// Drives a non-PTY tab as a full conversational backend (e.g. LM Studio).
    /// The tab routes send/read through the plugin instead of a terminal.
    /// Bool (registered, backend-only) or `{ spawnable }` object (user-openable).
    #[serde(default)]
    pub chat_backend: ChatBackendCap,
    /// Send-time text stage of the outbound pipeline (`chatPreprocess`). Matches
    /// final text and augments it (mem recall / worker / Groq) before transport.
    /// Bool or `{ match }` object (the §6 slot).
    #[serde(default)]
    pub chat_preprocessor: ChatPreprocessorCap,
    #[serde(default)]
    pub webhook_receiver: Option<WebhookReceiverCap>,
    /// Items the host renders in every tab's kebab menu; each action is host-owned.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tab_menu: Vec<TabMenuItem>,
}

impl Capabilities {
    /// Whether the `chatBackend` capability is a user-openable shell (the
    /// `{ spawnable: true }` form). Bare `chatBackend: true` is backend-only.
    pub fn chat_backend_spawnable(&self) -> bool {
        self.chat_backend.spawnable()
    }

    pub fn chat_backend_watcher_capable(&self) -> bool {
        self.chat_backend.watcher()
    }
}
