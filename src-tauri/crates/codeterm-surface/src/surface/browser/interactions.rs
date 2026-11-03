use super::{input::InputArgs, BrowserRuntime};
use codeterm_browser::PageChannel;
use serde_json::{json, Value};

#[derive(Debug, PartialEq)]
pub(super) enum Interaction {
    Target(Vec<String>),
    DragRefs(Vec<String>, Vec<String>),
    DragPoints([f64; 4]),
    Key(String, Vec<String>, usize),
    Form(Vec<String>, String),
}

pub(super) fn parse(verb: &str, args: &[String]) -> Result<Interaction, String> {
    let target = |args: &[String]| {
        if (args.len() == 1 && args[0].starts_with('@'))
            || (args.len() == 2 && !args[0].starts_with('-'))
        {
            Ok(args.to_vec())
        } else {
            Err("expected @ref or <role> <name>".to_string())
        }
    };
    match verb {
        "hover" | "double-click" | "triple-click" | "right-click" => {
            Ok(Interaction::Target(target(args)?))
        }
        "scroll-to" if args.len() == 1 && args[0].starts_with('@') => {
            Ok(Interaction::Target(args.to_vec()))
        }
        "drag" if args.len() == 2 && args.iter().all(|s| s.starts_with('@')) => Ok(
            Interaction::DragRefs(vec![args[0].clone()], vec![args[1].clone()]),
        ),
        "drag" if args.len() == 4 => {
            let mut points = [0.0; 4];
            for (slot, raw) in points.iter_mut().zip(args) {
                *slot = raw
                    .parse::<f64>()
                    .map_err(|_| "drag requires finite nonnegative coordinates")?;
                if !slot.is_finite() || *slot < 0.0 || *slot > 100_000.0 {
                    return Err("invalid drag coordinate".into());
                }
            }
            Ok(Interaction::DragPoints(points))
        }
        "form-value" if args.len() == 2 && args[0].starts_with('@') => {
            Ok(Interaction::Form(vec![args[0].clone()], args[1].clone()))
        }
        "key" => {
            let key = args
                .first()
                .filter(|s| !s.is_empty() && !s.starts_with('-'))
                .ok_or("key requires a key name")?
                .clone();
            let mut mods = Vec::new();
            let mut repeat = None;
            let mut i = 1;
            while i < args.len() {
                if args[i] == "--repeat" && repeat.is_none() {
                    i += 1;
                    let count = args
                        .get(i)
                        .and_then(|s| s.parse::<usize>().ok())
                        .filter(|n| (1..=100).contains(n))
                        .ok_or("repeat must be 1 to 100")?;
                    repeat = Some(count);
                } else if ["shift", "ctrl", "alt", "meta"].contains(&args[i].as_str()) {
                    mods.push(args[i].clone());
                } else {
                    return Err("unknown key option".into());
                }
                i += 1;
            }
            Ok(Interaction::Key(key, mods, repeat.unwrap_or(1)))
        }
        _ => Err(format!("invalid arguments for {verb}")),
    }
}

impl BrowserRuntime {
    async fn target_point(
        &self,
        page: &PageChannel,
        locator: &[String],
    ) -> Result<(f64, f64), String> {
        let target = self.resolve(locator)?;
        let quads = self
            .send(
                page,
                "DOM.getContentQuads",
                json!({"backendNodeId": target.backend_node_id}),
            )
            .await?;
        let current = self.resolve(locator)?;
        if current.backend_node_id != target.backend_node_id {
            return Err("stale: target changed".into());
        }
        super::input::quad_center(&quads).ok_or("hidden: target has no box".into())
    }

    pub(super) async fn interaction_verb(
        &self,
        page: &PageChannel,
        verb: &str,
        args: &[String],
    ) -> Result<String, String> {
        let request = parse(verb, args)?;
        let _guard = self.action_gate.lock().await;
        if let Interaction::Form(locator, value) = &request {
            return self.element_action(page, locator, "form", value).await;
        }
        if verb == "scroll-to" {
            return self.element_action(page, args, "scroll", "").await;
        }
        let epoch = self
            .evaluate(page, include_str!("pointer-begin.js"), false)
            .await?;
        let mut commands = Vec::new();
        match &request {
            Interaction::Key(key, mods, repeat) => {
                for _ in 0..*repeat {
                    commands.extend(super::input::input_commands(&InputArgs::key(key, mods))?);
                }
            }
            Interaction::Target(locator) => {
                let (x, y) = self.target_point(page, locator).await?;
                if verb == "hover" {
                    commands.push((
                        "Input.dispatchMouseEvent",
                        json!({"type":"mouseMoved","x":x,"y":y,"button":"none"}),
                    ));
                } else {
                    let count = match verb {
                        "double-click" => 2,
                        "triple-click" => 3,
                        _ => 1,
                    };
                    for click_count in 1..=count {
                        let mut input = InputArgs::click(x, y);
                        input.click_count = Some(click_count);
                        input.button = Some(
                            if verb == "right-click" {
                                "right"
                            } else {
                                "left"
                            }
                            .into(),
                        );
                        commands.extend(super::input::input_commands(&input)?);
                    }
                }
            }
            Interaction::DragRefs(_, _) | Interaction::DragPoints(_) => {
                let (x, y, end_x, end_y) = match &request {
                    Interaction::DragRefs(from, to) => {
                        let (x, y) = self.target_point(page, from).await?;
                        let (end_x, end_y) = self.target_point(page, to).await?;
                        self.resolve(from)?;
                        (x, y, end_x, end_y)
                    }
                    Interaction::DragPoints([x, y, end_x, end_y]) => (*x, *y, *end_x, *end_y),
                    _ => unreachable!(),
                };
                for (kind, x, y, buttons) in [
                    ("mouseMoved", x, y, 0),
                    ("mousePressed", x, y, 1),
                    ("mouseMoved", end_x, end_y, 1),
                    ("mouseReleased", end_x, end_y, 0),
                ] {
                    commands.push(("Input.dispatchMouseEvent", json!({"type":kind,"x":x,"y":y,"button":"left","buttons":buttons,"clickCount":1})));
                }
            }
            _ => return Err("unsupported interaction".into()),
        }
        let result = self
            .complete_action(page, false, async {
                if self
                    .evaluate(
                        page,
                        &format!("{}({})", include_str!("pointer-check.js"), epoch),
                        false,
                    )
                    .await?
                    != Value::Bool(true)
                {
                    return Err("stale: page changed while preparing interaction".into());
                }
                page.send_batch(commands).await.map_err(super::cdp_error)?;
                Ok(Value::Null)
            })
            .await;
        self.shared.invalidate_snapshot();
        result.map(|(_, completion)| {
            format!("{verb}: {completion}; re-snapshot to verify the result")
        })
    }

    async fn element_action(
        &self,
        page: &PageChannel,
        locator: &[String],
        action: &str,
        value: &str,
    ) -> Result<String, String> {
        let target = self.resolve(locator)?;
        let epoch = self
            .evaluate(page, include_str!("atomic-begin.js"), false)
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
            .ok_or("stale: detached element")?;
        self.resolve(locator)?;
        let result = self.complete_action(page,false,self.send(page,"Runtime.callFunctionOn",json!({"objectId":id,"functionDeclaration":include_str!("element-action.js"),"arguments":[{"value":epoch},{"value":action},{"value":value}],"returnByValue":true}))).await;
        let _ = self
            .send(page, "Runtime.releaseObject", json!({"objectId":id}))
            .await;
        self.shared.invalidate_snapshot();
        let (reply, completion) = result?;
        match reply.pointer("/result/value").and_then(Value::as_str) {
            Some("performed") => Ok(format!("{completion}; re-snapshot to verify the value")),
            Some(reason) => Err(format!("{reason}: element action refused")),
            None => Err("action_unconfirmed: inspect before continuing; never replay".into()),
        }
    }
}

#[cfg(test)]
mod tests;
