use crate::client::SocketClient;
use crate::cmd::mem::item::{self, SaveMemItem};
use crate::MemAction;

/// Every `MemAction` the daemon has socket parity for is dispatched here and
/// short-circuits the CLI's legacy HTTP transport entirely — a live socket
/// (guaranteed by the time `handle_mem` runs; see `main.rs`) is the only
/// path. `None` means the action belongs to a subsystem (skill/graph/worker/
/// calibration/secret) that still speaks HTTP; `MemAction::History` /
/// `Taxonomy` / `Tail` are handled directly by their own CLI modules over
/// this same socket client instead of going through this generic path, so
/// their pretty-printing / `--follow` behavior is preserved.
pub(super) fn try_dispatch(
    client: &mut SocketClient,
    action: &MemAction,
) -> Option<Result<serde_json::Value, String>> {
    let (method, params) = socket_request(action)?;
    Some(client.call(method, params))
}

pub(super) fn socket_request(
    action: &MemAction,
) -> Option<(&'static str, Option<serde_json::Value>)> {
    match action {
        MemAction::Stats => Some(("mem.stats", None)),
        MemAction::Search {
            query,
            kind,
            area,
            scope,
            project,
            limit,
            rerank,
            decision_rerank,
        } => {
            let project = project.clone().or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|path| path.display().to_string())
            });
            let mut params = serde_json::json!({
                "q": query,
                "kind": kind,
                "area": area,
                "scope": scope,
                "project": project,
                "limit": limit,
                "rerank": rerank,
            });
            if let Some(mode) = decision_rerank {
                params["decision_rerank"] = serde_json::json!(mode.enabled());
            }
            Some(("mem.search", Some(params)))
        }
        MemAction::Get { id } => Some(("mem.get", Some(serde_json::json!({ "id": id })))),
        MemAction::Delete { id } => Some(("mem.delete", Some(serde_json::json!({ "id": id })))),
        MemAction::Forget { area, yes } => {
            if let Err(e) = item::validate_mem_forget_confirmation(area, *yes) {
                crate::cmd::print::fail(1, e.to_string());
            }
            Some(("mem.forget", Some(serde_json::json!({ "area": area }))))
        }
        MemAction::Update {
            id,
            title,
            summary,
            area,
            tags,
            body,
            body_file,
            importance,
            changed_by,
        } => {
            let body = resolve_or_exit(body.clone(), body_file.clone());
            let tag_list = tags.clone().map(|t| item::split_csv(Some(t)));
            Some((
                "mem.update",
                Some(serde_json::json!({
                    "id": id,
                    "title": title,
                    "summary": summary,
                    "area": area,
                    "tags": tag_list,
                    "body": body,
                    "importance": importance,
                    "changed_by": changed_by,
                })),
            ))
        }
        MemAction::Pin { id } => Some((
            "mem.update",
            Some(serde_json::json!({ "id": id, "pinned": true })),
        )),
        MemAction::Unpin { id } => Some((
            "mem.update",
            Some(serde_json::json!({ "id": id, "pinned": false })),
        )),
        MemAction::Pinned { limit } => {
            Some(("mem.pinned", Some(serde_json::json!({ "limit": limit }))))
        }
        MemAction::Save {
            kind,
            title,
            body,
            body_file,
            summary,
            tags,
            area,
            importance,
            scope,
            workspace,
            pin,
            source_path,
            replace,
            skip_extract,
        } => Some((
            "mem.save",
            Some(item::build_save_payload(SaveMemItem {
                kind: kind.clone(),
                title: title.clone(),
                body: body.clone(),
                body_file: body_file.clone(),
                summary: summary.clone(),
                tags: tags.clone(),
                area: area.clone(),
                importance: *importance,
                scope: scope.clone(),
                workspace: workspace.clone(),
                pin: *pin,
                source_path: source_path.clone(),
                replace: *replace,
                skip_extract: *skip_extract,
            })),
        )),
        MemAction::SavePreference {
            title,
            body,
            body_file,
            summary,
            tags,
            area,
            importance,
            scope,
            workspace,
            pin,
            replace,
        } => Some((
            "mem.save",
            Some(item::build_save_payload(SaveMemItem {
                kind: "preference".to_string(),
                title: title.clone(),
                body: body.clone(),
                body_file: body_file.clone(),
                summary: summary.clone(),
                tags: tags.clone(),
                area: area.clone(),
                importance: *importance,
                scope: scope.clone(),
                workspace: workspace.clone(),
                pin: *pin,
                source_path: None,
                replace: *replace,
                skip_extract: true,
            })),
        )),
        MemAction::ExtractPending { limit } => Some((
            "mem.extract_pending",
            Some(serde_json::json!({ "limit": limit })),
        )),
        MemAction::WakeUp {
            budget,
            l0_only,
            no_l0,
            explain,
            workspace,
        } => {
            let workspace = item::resolve_wakeup_workspace(
                workspace.clone(),
                std::env::var("CODETERM_WORKSPACE_ID").ok(),
            );
            Some((
                "mem.wake_up",
                Some(serde_json::json!({
                    "budget": budget,
                    "l0_only": l0_only,
                    "no_l0": no_l0,
                    "explain": explain,
                    "workspace": workspace,
                })),
            ))
        }
        MemAction::Learn {
            text,
            kind,
            tags,
            error,
            feature,
        } => {
            let (resolved_text, resolved_kind) = if let Some(t) = error {
                (t.clone(), "error".to_string())
            } else if let Some(t) = feature {
                (t.clone(), "feature".to_string())
            } else if let Some(t) = text {
                (t.clone(), kind.clone().unwrap_or_else(|| "insight".into()))
            } else {
                use std::io::Read;
                let mut s = String::new();
                if std::io::stdin().read_to_string(&mut s).is_err() || s.trim().is_empty() {
                    crate::cmd::print::fail(
                        1,
                        "provide text as arg, --error/--feature, or pipe to stdin",
                    );
                }
                (s, kind.clone().unwrap_or_else(|| "insight".into()))
            };
            let tag_list = item::split_csv(tags.clone());
            Some((
                "mem.learn",
                Some(serde_json::json!({
                    "text": resolved_text, "kind": resolved_kind, "tags": tag_list,
                })),
            ))
        }
        MemAction::LearnList => Some((
            "mem.search",
            Some(serde_json::json!({ "q": "learning", "kind": "learning", "limit": 50 })),
        )),
        MemAction::LearnPromote { id } => {
            Some(("mem.learn_promote", Some(serde_json::json!({ "id": id }))))
        }
        MemAction::Flush { tab, title } => {
            let tab_id = tab
                .clone()
                .or_else(|| std::env::var("CODETERM_TAB_ID").ok())
                .unwrap_or_else(|| "current".to_string());
            Some((
                "mem.flush",
                Some(serde_json::json!({ "tab": tab_id, "title": title })),
            ))
        }
        MemAction::Sync { path } => Some(("mem.sync", Some(serde_json::json!({ "path": path })))),
        MemAction::Append {
            id,
            text,
            file,
            changed_by,
        } => {
            let resolved = match item::resolve_text(text.clone(), file.clone()) {
                Ok(Some(t)) => t,
                Ok(None) => match item::read_path_or_stdin("-") {
                    Ok(s) if !s.trim().is_empty() => s,
                    _ => {
                        crate::cmd::print::fail(
                            1,
                            "no text provided (pass as arg, --file, or pipe to stdin)",
                        );
                    }
                },
                Err(e) => {
                    crate::cmd::print::fail(1, e.to_string());
                }
            };
            Some((
                "mem.append",
                Some(serde_json::json!({ "id": id, "text": resolved, "changed_by": changed_by })),
            ))
        }
        MemAction::Restore {
            id,
            version,
            changed_by,
        } => Some((
            "mem.restore",
            Some(serde_json::json!({ "id": id, "version": version, "changed_by": changed_by })),
        )),
        MemAction::Reembed { scope } => {
            Some(("mem.reembed", Some(serde_json::json!({ "scope": scope }))))
        }
        _ => None,
    }
}

fn resolve_or_exit(value: Option<String>, file: Option<String>) -> Option<String> {
    match item::resolve_text(value, file) {
        Ok(v) => v,
        Err(e) => {
            crate::cmd::print::fail(1, e.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::socket_request;
    use crate::{Cli, Commands, MemAction};
    use clap::Parser;

    #[test]
    fn observed_issue_actions_are_socket_routed() {
        for args in [
            vec!["codeterm", "mem", "stats"],
            vec!["codeterm", "mem", "search", "test", "--limit", "1"],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            let Commands::Mem { action } = cli.command else {
                panic!("expected mem command");
            };
            let (method, params) = socket_request(&action).expect("socket-routed mem action");
            match action {
                MemAction::Stats => {
                    assert_eq!(method, "mem.stats");
                    assert!(params.is_none());
                }
                MemAction::Search { .. } => {
                    assert_eq!(method, "mem.search");
                    let params = params.unwrap();
                    assert_eq!(params["q"], "test");
                    assert_eq!(params["limit"], 1);
                }
                _ => panic!("unexpected mem action"),
            }
        }
    }

    #[test]
    fn full_item_surface_is_socket_routed() {
        let cases: &[(Vec<&str>, &str)] = &[
            (vec!["codeterm", "mem", "get", "fact_a"], "mem.get"),
            (vec!["codeterm", "mem", "delete", "fact_a"], "mem.delete"),
            (
                vec!["codeterm", "mem", "forget", "auth", "--yes"],
                "mem.forget",
            ),
            (
                vec!["codeterm", "mem", "update", "fact_a", "--importance", "4"],
                "mem.update",
            ),
            (vec!["codeterm", "mem", "pin", "fact_a"], "mem.update"),
            (vec!["codeterm", "mem", "unpin", "fact_a"], "mem.update"),
            (vec!["codeterm", "mem", "pinned"], "mem.pinned"),
            (
                vec![
                    "codeterm", "mem", "save", "--kind", "fact", "--title", "t", "body",
                ],
                "mem.save",
            ),
            (
                vec!["codeterm", "mem", "save-preference", "--title", "t", "body"],
                "mem.save",
            ),
            (
                vec!["codeterm", "mem", "extract-pending"],
                "mem.extract_pending",
            ),
            (vec!["codeterm", "mem", "wake-up"], "mem.wake_up"),
            (
                vec!["codeterm", "mem", "learn", "insight text"],
                "mem.learn",
            ),
            (vec!["codeterm", "mem", "learn-list"], "mem.search"),
            (
                vec!["codeterm", "mem", "learn-promote", "learning_a"],
                "mem.learn_promote",
            ),
            (
                vec!["codeterm", "mem", "flush", "--tab", "tab1"],
                "mem.flush",
            ),
            (vec!["codeterm", "mem", "sync"], "mem.sync"),
            (
                vec!["codeterm", "mem", "append", "fact_a", "more text"],
                "mem.append",
            ),
            (vec!["codeterm", "mem", "restore", "fact_a"], "mem.restore"),
            (vec!["codeterm", "mem", "reembed"], "mem.reembed"),
        ];
        for (args, expected_method) in cases {
            let cli = Cli::try_parse_from(args.clone()).unwrap();
            let Commands::Mem { action } = cli.command else {
                panic!("expected mem command for {args:?}");
            };
            let (method, _params) =
                socket_request(&action).unwrap_or_else(|| panic!("not socket-routed: {args:?}"));
            assert_eq!(method, *expected_method, "mismatch for {args:?}");
        }
    }

    #[test]
    fn subsystem_actions_stay_off_socket_dispatch() {
        for args in [
            vec!["codeterm", "mem", "history", "fact_a"],
            vec!["codeterm", "mem", "taxonomy"],
            vec!["codeterm", "mem", "tail"],
            vec!["codeterm", "mem", "skill", "list"],
            vec!["codeterm", "mem", "worker", "list"],
        ] {
            let cli = Cli::try_parse_from(args.clone()).unwrap();
            let Commands::Mem { action } = cli.command else {
                panic!("expected mem command for {args:?}");
            };
            assert!(
                socket_request(&action).is_none(),
                "{args:?} should not be generically socket-routed"
            );
        }
    }
}
