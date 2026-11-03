use crate::{cdp_wire::CdpEvent, EventStream, PageChannel};
use serde_json::{json, Value};
use std::{collections::HashSet, future::Future, time::Duration};

pub struct ActionCompletion {
    events: EventStream,
    frame: String,
    previous_loader: String,
    session: String,
    target: String,
    requested: bool,
    loader: Option<String>,
    loaded: HashSet<String>,
    url: Option<String>,
    same_document: bool,
    restored: bool,
}

impl ActionCompletion {
    pub async fn observe(page: &PageChannel) -> Result<Self, String> {
        page.send("Page.enable", json!({}))
            .await
            .map_err(|e| e.to_string())?;
        page.send("Page.setLifecycleEventsEnabled", json!({"enabled":true}))
            .await
            .map_err(|e| e.to_string())?;
        let tree = page
            .send("Page.getFrameTree", json!({}))
            .await
            .map_err(|e| e.to_string())?;
        let frame = tree
            .pointer("/frameTree/frame")
            .ok_or("missing main frame")?;
        Ok(Self {
            events: page.subscribe(&[
                "Page.frameRequestedNavigation",
                "Page.frameStartedNavigating",
                "Page.frameNavigated",
                "Page.navigatedWithinDocument",
                "Page.lifecycleEvent",
                "Inspector.detached",
                "Target.detachedFromTarget",
                "Target.targetDestroyed",
            ]),
            frame: frame["id"].as_str().ok_or("missing main frame id")?.into(),
            previous_loader: frame["loaderId"].as_str().unwrap_or_default().into(),
            session: page.session_id().into(),
            target: page.target_id().into(),
            requested: false,
            loader: None,
            loaded: HashSet::new(),
            url: None,
            same_document: false,
            restored: false,
        })
    }

    fn event(&mut self, event: &CdpEvent) -> Result<(), String> {
        let p = &event.params;
        if (event.method == "Target.targetDestroyed" && p["targetId"] == self.target)
            || (event.method == "Target.detachedFromTarget" && p["sessionId"] == self.session)
            || (event.method == "Inspector.detached"
                && event.session_id.as_deref() == Some(&self.session))
        {
            return Err(
                "action_unconfirmed: page closed; inspect before continuing; never replay".into(),
            );
        }
        if event.session_id.as_deref() != Some(&self.session) {
            return Ok(());
        }
        match event.method.as_str() {
            "Page.frameRequestedNavigation" if p["frameId"] == self.frame => {
                self.requested = true;
                self.same_document = false;
            }
            "Page.frameStartedNavigating" if p["frameId"] == self.frame => {
                self.requested = !matches!(
                    p["navigationType"].as_str(),
                    Some("sameDocument" | "historySameDocument")
                );
                self.same_document = false;
            }
            "Page.frameNavigated"
                if p["frame"]["id"] == self.frame && p["frame"]["parentId"].is_null() =>
            {
                if let Some(url) = p["frame"]["unreachableUrl"]
                    .as_str()
                    .filter(|url| !url.is_empty())
                {
                    return Err(format!("navigation failed: unreachable {url}"));
                }
                let loader = p["frame"]["loaderId"]
                    .as_str()
                    .ok_or("missing navigation loader")?;
                if loader != self.previous_loader {
                    if self.loader.as_deref().is_some_and(|own| own != loader) {
                        return Err(
                            "action_unconfirmed: navigation superseded; never replay".into()
                        );
                    }
                    self.requested = true;
                    self.loader = Some(loader.into());
                    self.restored = p["type"] == "BackForwardCacheRestore";
                    self.url = p["frame"]["url"].as_str().map(|url| {
                        format!(
                            "{url}{}",
                            p["frame"]["urlFragment"].as_str().unwrap_or_default()
                        )
                    });
                }
            }
            "Page.navigatedWithinDocument" if p["frameId"] == self.frame => {
                self.same_document = true;
                self.url = p["url"].as_str().map(str::to_string);
            }
            "Page.lifecycleEvent" if p["frameId"] == self.frame && p["name"] == "load" => {
                if let Some(loader) = p["loaderId"].as_str() {
                    if self.loaded.len() >= 32 {
                        return Err("action_unconfirmed: navigation event limit exceeded".into());
                    }
                    self.loaded.insert(loader.into());
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Acknowledgement proves synchronous action dispatch only. Later asynchronous effects
    /// require explicit state waits. An observed main-frame navigation requires its own load.
    pub async fn finish<F>(
        mut self,
        action: F,
        navigation_required: bool,
        deadline: Duration,
    ) -> Result<(Value, String), String>
    where
        F: Future<Output = Result<Value, String>>,
    {
        tokio::time::timeout(deadline, async {
            tokio::pin!(action);
            let reply = loop {
                tokio::select! {
                    biased;
                    event = self.events.recv() => {
                        let event = event.ok_or("action_unconfirmed: browser observation closed")?;
                        self.event(&event)?;
                    },
                    reply = &mut action => break reply.map_err(|e| format!("action_unconfirmed: {e}; never replay"))?,
                }
            };
            while let Ok(event) = self.events.try_recv() {
                self.event(&event)?;
            }
            if let Some(error) = reply["errorText"].as_str().filter(|error| !error.is_empty()) {
                return Err(format!("navigation failed: {error}"));
            }
            if let Some(loader) = reply["loaderId"].as_str() {
                if self.loader.as_deref().is_some_and(|observed| observed != loader) {
                    return Err("action_unconfirmed: navigation superseded; never replay".into());
                }
                self.loader = Some(loader.into());
                self.requested = true;
            }
            loop {
                if self.events.is_closed() || self.events.has_lost_events() {
                    return Err("action_unconfirmed: browser observation closed or overflowed".into());
                }
                if self.loader.as_ref().is_some_and(|loader| self.loaded.contains(loader) || self.restored) {
                    let boundary = if self.restored { "restored" } else { "loaded" };
                    return Ok((reply, format!("navigation {boundary}: {}", self.url.as_deref().unwrap_or("(unknown URL)"))));
                }
                if self.same_document && self.loader.is_none() && !self.requested {
                    return Ok((reply, format!("same-document navigation: {}", self.url.as_deref().unwrap_or("(unknown URL)"))));
                }
                if !navigation_required && !self.requested {
                    return Ok((reply, "action acknowledged; asynchronous effects require an explicit state wait".into()));
                }
                let event = self.events.recv().await.ok_or("action_unconfirmed: browser observation closed")?;
                self.event(&event)?;
            }
        }).await.map_err(|_| "action_unconfirmed: completion deadline exceeded; inspect before continuing; never replay".to_string())?
    }
}

#[cfg(test)]
mod tests;
