//! Questions a peer Main puts to its own owner before hosting for another machine: a template
//! start or a clone. Each is an ordinary daemon question; the answer runs on this peer and its
//! outcome waits here until the origin reads it.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use codeterm_team_package::contracts::TemplatePeerStartAsk;
use parking_lot::Mutex;
use serde_json::Value;

/// The daemon question request prefix every peer-hosting ask carries.
pub const PEER_ASK_PREFIX: &str = "peer-hosting:";
pub const START_ACCEPT: &str = "Start here";
pub const CLONE_ACCEPT: &str = "Clone";
pub const PEER_ASK_DECLINE: &str = "Decline";

/// Bounds both the open asks and the outcomes kept for origins; the oldest goes first.
const MAX_ASKS: usize = 64;

#[derive(Debug, Clone)]
pub enum PeerAsk {
    Start {
        origin_peer: String,
        ask: TemplatePeerStartAsk,
    },
    Clone {
        origin_peer: String,
        url: String,
        path: PathBuf,
    },
}

#[derive(Default)]
struct AskState {
    open: VecDeque<(String, PeerAsk)>,
    outcomes: VecDeque<(String, Value)>,
}

/// Open asks and finished outcomes. An answer takes its ask exactly once, so a second tap or
/// a repeated origin request never runs the same start or clone twice.
#[derive(Default)]
pub struct PeerAsks {
    state: Mutex<AskState>,
}

impl PeerAsks {
    pub fn global() -> &'static Self {
        static ASKS: OnceLock<PeerAsks> = OnceLock::new();
        ASKS.get_or_init(Self::default)
    }

    /// Records an ask; the returned request id names its daemon question.
    pub fn open(&self, ask: PeerAsk) -> String {
        let request = format!("{PEER_ASK_PREFIX}{}", uuid::Uuid::new_v4().simple());
        let mut state = self.state.lock();
        if state.open.len() == MAX_ASKS {
            state.open.pop_front();
        }
        state.open.push_back((request.clone(), ask));
        request
    }

    /// An open clone of the same URL is asked once; a repeated dispatch points at it.
    pub fn open_clone(&self, url: &str) -> Option<String> {
        self.state
            .lock()
            .open
            .iter()
            .find(|(_, ask)| matches!(ask, PeerAsk::Clone { url: open, .. } if open == url))
            .map(|(request, _)| request.clone())
    }

    pub fn take(&self, request: &str) -> Option<PeerAsk> {
        let mut state = self.state.lock();
        let index = state.open.iter().position(|(id, _)| id == request)?;
        state.open.remove(index).map(|(_, ask)| ask)
    }

    pub fn finish(&self, request: &str, outcome: Value) {
        let mut state = self.state.lock();
        state.outcomes.retain(|(id, _)| id != request);
        if state.outcomes.len() == MAX_ASKS {
            state.outcomes.pop_front();
        }
        state.outcomes.push_back((request.to_string(), outcome));
    }

    /// The finished outcome, or `awaiting_owner` while the ask is still open.
    pub fn outcome(&self, request: &str) -> Option<Value> {
        let state = self.state.lock();
        if let Some((_, outcome)) = state.outcomes.iter().find(|(id, _)| id == request) {
            return Some(outcome.clone());
        }
        state
            .open
            .iter()
            .any(|(id, _)| id == request)
            .then(|| serde_json::json!({ "state": "awaiting_owner" }))
    }
}

/// A remote URL another machine may clone, with every credential removed: userinfo is dropped
/// (an ssh login name is kept, never a password), as are query and fragment. Local paths and
/// unknown schemes are refused, since they mean nothing on the peer.
pub fn scrub_remote_url(remote: &str) -> Option<String> {
    let remote = remote.trim();
    if remote.is_empty() || remote.chars().any(char::is_whitespace) {
        return None;
    }
    if let Some((scheme, rest)) = remote.split_once("://") {
        let scheme = scheme.to_ascii_lowercase();
        if !matches!(scheme.as_str(), "https" | "http" | "ssh" | "git") {
            return None;
        }
        let rest = rest.split(['?', '#']).next().unwrap_or_default();
        let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
        let (userinfo, host) = match authority.rsplit_once('@') {
            Some((userinfo, host)) => (Some(userinfo), host),
            None => (None, authority),
        };
        if host.is_empty() || path.trim_matches('/').is_empty() {
            return None;
        }
        let login = match (scheme.as_str(), userinfo) {
            ("ssh", Some(userinfo)) => userinfo
                .split(':')
                .next()
                .filter(|user| !user.is_empty())
                .map(|user| format!("{user}@"))
                .unwrap_or_default(),
            _ => String::new(),
        };
        return Some(format!("{scheme}://{login}{host}{path}"));
    }
    let (user, host_path) = remote.split_once('@')?;
    let (host, path) = host_path.split_once(':')?;
    if user.is_empty()
        || user.contains(':')
        || user.contains('/')
        || host.is_empty()
        || host.contains('/')
        || path.trim_matches('/').is_empty()
    {
        return None;
    }
    Some(remote.to_string())
}

/// The origin checkout's credential-free clone URL for a peer, the `origin` remote first.
pub fn origin_clone_url(cwd: &str) -> Option<String> {
    use crate::service::worker_lane::git_value;
    let remotes = git_value(cwd, ["remote"]).ok()?;
    let mut names: Vec<&str> = remotes
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    names.sort_by_key(|name| *name != "origin");
    names.into_iter().find_map(|name| {
        let url = git_value(cwd, ["remote", "get-url", name]).ok()?;
        scrub_remote_url(&url)
    })
}

/// Where a clone lands under `root`: the repository's own name, never a path it chose.
pub fn clone_destination(root: &Path, url: &str) -> Option<PathBuf> {
    let last = url
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()?
        .trim_end_matches(".git");
    let valid = !last.is_empty()
        && last != "."
        && last != ".."
        && last
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    valid.then(|| root.join(last))
}
