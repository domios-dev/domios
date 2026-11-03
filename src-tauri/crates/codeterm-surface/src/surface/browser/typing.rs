use super::{input::InputArgs, BrowserRuntime};
use codeterm_browser::PageChannel;
use serde_json::{json, Value};

pub(super) fn parse(args: &[String], secret: bool) -> Result<(Vec<String>, bool), String> {
    let mut words = args.to_vec();
    let flag = words.iter().enumerate().find_map(|(index, word)| {
        (word == "--submit" && (!secret || index + 1 < words.len())).then_some(index)
    });
    let submit = flag.is_some();
    if let Some(index) = flag {
        words.remove(index);
    }
    if !(2..=3).contains(&words.len()) {
        return Err("usage: type [--submit] @ref|<name>|<role> <name> <text>".into());
    }
    Ok((words, submit))
}

impl BrowserRuntime {
    pub(super) async fn type_verb(
        &self,
        page: &PageChannel,
        args: &[String],
        secret: bool,
    ) -> Result<String, String> {
        let (words, submit) = parse(args, secret)?;
        let (text, locator) = words.split_last().ok_or("missing text")?;
        let _guard = self.action_gate.lock().await;
        let target = self.visible_target(page, locator, true).await?;
        if !submit {
            self.type_into(page, target.backend_node_id, text, secret)
                .await?;
            return Ok(format!("typed into {}", target.label));
        }
        let (value, completion) = self
            .complete_action(page, false, async {
                self.type_into(page, target.backend_node_id, text, secret)
                    .await?;
                let object = self
                    .send(
                        page,
                        "DOM.resolveNode",
                        json!({"backendNodeId":target.backend_node_id}),
                    )
                    .await?;
                let id = object
                    .pointer("/object/objectId")
                    .and_then(Value::as_str)
                    .ok_or("submit_unconfirmed: field detached; never replay typing")?;
                let key = format!("domiosSubmit{}", uuid::Uuid::new_v4().simple());
                let result = async {
                    let ready = self.submit_stage(page, id, "begin", &key).await?;
                    if ready != "ready" {
                        return Err(format!("submit_unconfirmed: {ready}; never replay typing"));
                    }
                    let frame = self.send(page, "Page.getFrameTree", json!({})).await?;
                    let main = frame
                        .pointer("/frameTree/frame/id")
                        .and_then(Value::as_str)
                        .ok_or("submit_unconfirmed: missing main frame")?;
                    let mut navigation = page.subscribe_session(&[
                        "Page.frameRequestedNavigation",
                        "Page.frameStartedNavigating",
                        "Page.frameNavigated",
                        "Page.navigatedWithinDocument",
                    ]);
                    self.dispatch_input(page, &InputArgs::key("Enter", &[]))
                        .await?;
                    let mut navigated = false;
                    while let Ok(event) = navigation.try_recv() {
                        navigated |=
                            event.params["frameId"] == main || event.params["frame"]["id"] == main;
                    }
                    if navigation.is_closed() || navigation.has_lost_events() {
                        return Err(
                            "submit_unconfirmed: navigation observation lost; never replay typing"
                                .into(),
                        );
                    }
                    if navigated {
                        return Ok(json!({"submitObserved":false}));
                    }
                    let evidence = self.submit_stage(page, id, "finish", &key).await?;
                    Ok(json!({"submitObserved":evidence == "submitted"}))
                }
                .await;
                let _ = self.submit_stage(page, id, "cleanup", &key).await;
                let _ = self
                    .send(page, "Runtime.releaseObject", json!({"objectId":id}))
                    .await;
                result
            })
            .await?;
        self.shared.invalidate_snapshot();
        if value["submitObserved"] == true || completion.contains("navigation") {
            Ok(format!(
                "typed into {}; submit evidence: {}; inspect the resulting page",
                target.label,
                if value["submitObserved"] == true {
                    "form submit event"
                } else {
                    &completion
                }
            ))
        } else {
            Err("submit_unconfirmed: text was inserted but no navigation or submit event was observed; inspect the page; never replay typing".into())
        }
    }

    async fn submit_stage(
        &self,
        page: &PageChannel,
        id: &str,
        stage: &str,
        key: &str,
    ) -> Result<String, String> {
        let reply = self.send(page, "Runtime.callFunctionOn", json!({"objectId":id,"functionDeclaration":include_str!("submit.js"),"arguments":[{"value":stage},{"value":key}],"returnByValue":true})).await?;
        reply.pointer("/result/value").and_then(Value::as_str).map(str::to_string).ok_or("submit_unconfirmed: submission context changed; inspect before continuing; never replay typing".into())
    }

    pub(super) async fn type_into(
        &self,
        page: &PageChannel,
        backend_node_id: i64,
        text: &str,
        secret: bool,
    ) -> Result<(), String> {
        let object = self
            .send(
                page,
                "DOM.resolveNode",
                json!({"backendNodeId":backend_node_id}),
            )
            .await?;
        let id = object
            .pointer("/object/objectId")
            .and_then(Value::as_str)
            .ok_or("stale: field detached")?;
        let result = async {
            let reply = self.send(page, "Runtime.callFunctionOn", json!({"objectId":id,"functionDeclaration":include_str!("type-focus.js"),"arguments":[{"value":secret}],"returnByValue":true})).await?;
            match reply.pointer("/result/value").and_then(Value::as_str) {
                Some("focused") => {},
                Some(reason) => return Err(format!("{reason}: typing refused before insertion")),
                None => return Err("action_unconfirmed: field preparation failed; inspect before continuing".into()),
            }
            self.send(page, "Input.insertText", json!({"text":text})).await.map(|_| ()).map_err(|_| "action_unconfirmed: text insertion may have occurred; inspect before continuing; never replay".into())
        }.await;
        let _ = self
            .send(page, "Runtime.releaseObject", json!({"objectId":id}))
            .await;
        result
    }
}

#[cfg(test)]
mod tests;
