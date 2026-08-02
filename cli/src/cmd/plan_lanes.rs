use serde_json::Value;

struct LaneRow {
    task: String,
    status: String,
    tab: String,
    branch: String,
    worktree: String,
    cleanup: String,
    stale: bool,
}

fn opt<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
}

// Liveness verdict -> short tab state; keeps peer_offline distinct from dead.
fn tab_state(liveness: &str) -> &'static str {
    match liveness {
        "working" => "alive",
        "report_overdue" => "overdue",
        "idle_stalled" => "stalled",
        "tab_gone" => "dead",
        "peer_offline" => "peer-off",
        _ => "-",
    }
}

fn is_stale(liveness: &str, uncleaned: bool) -> bool {
    matches!(liveness, "tab_gone" | "report_overdue") || uncleaned
}

fn task_label(v: &Value) -> String {
    let handle = opt(v, "handle");
    let id = opt(v, "task_id").unwrap_or("?");
    let title = opt(v, "title").unwrap_or("");
    let head = handle.unwrap_or(id);
    if title.is_empty() {
        head.to_string()
    } else {
        format!("{head} {title}")
    }
}

fn cleanup_label(v: &Value) -> String {
    let uncleaned = v
        .get("uncleaned")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    match opt(v, "cleanup_status") {
        Some(s) => s.to_string(),
        None if uncleaned => "uncleaned".to_string(),
        None => "-".to_string(),
    }
}

fn build_rows(val: &Value) -> Vec<LaneRow> {
    let Some(lanes) = val.get("lanes").and_then(|v| v.as_array()) else {
        return vec![];
    };
    lanes
        .iter()
        .map(|v| {
            let liveness = opt(v, "liveness").unwrap_or("no_tab");
            let uncleaned = v
                .get("uncleaned")
                .and_then(|x| x.as_bool())
                .unwrap_or(false);
            LaneRow {
                task: task_label(v),
                status: opt(v, "status").unwrap_or("-").to_string(),
                tab: tab_state(liveness).to_string(),
                branch: opt(v, "branch").unwrap_or("-").to_string(),
                worktree: opt(v, "worktree").unwrap_or("-").to_string(),
                cleanup: cleanup_label(v),
                stale: is_stale(liveness, uncleaned),
            }
        })
        .collect()
}

fn pad(s: &str, w: usize) -> String {
    format!("{s:<w$}")
}

pub fn render_lanes(val: &Value, stale_only: bool) -> String {
    let mut rows = build_rows(val);
    if stale_only {
        rows.retain(|r| r.stale);
    }
    if rows.is_empty() {
        return if stale_only {
            "no stale lanes\n".to_string()
        } else if val
            .get("lane_history_count")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            > 0
        {
            "all registered lanes are closed\n".to_string()
        } else {
            "no lanes have been registered\n".to_string()
        };
    }

    let headers = ["TASK", "STATUS", "TAB", "BRANCH", "WORKTREE", "CLEANUP"];
    let mut w = headers.map(|h| h.len());
    for r in &rows {
        let cells = [
            &r.task,
            &r.status,
            &r.tab,
            &r.branch,
            &r.worktree,
            &r.cleanup,
        ];
        for (i, c) in cells.iter().enumerate() {
            w[i] = w[i].max(c.len());
        }
    }

    let mut out = String::new();
    for (i, h) in headers.iter().enumerate() {
        out.push_str(&pad(h, w[i]));
        out.push_str(if i + 1 < headers.len() { "  " } else { "\n" });
    }
    for r in &rows {
        let cells = [
            &r.task,
            &r.status,
            &r.tab,
            &r.branch,
            &r.worktree,
            &r.cleanup,
        ];
        for (i, c) in cells.iter().enumerate() {
            out.push_str(&pad(c, w[i]));
            out.push_str(if i + 1 < cells.len() { "  " } else { "\n" });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Value {
        json!({
            "lanes": [
                {
                    "task_id": "t_alive", "handle": "t_alive", "title": "live work",
                    "status": "in_progress", "tab_id": "tab-a", "liveness": "working",
                    "branch": "lane/a", "worktree": "/wt/a",
                    "cleanup_status": null, "uncleaned": false
                },
                {
                    "task_id": "t_dead", "handle": "t_dead", "title": "gone",
                    "status": "in_progress", "tab_id": "tab-d", "liveness": "tab_gone",
                    "branch": "lane/d", "worktree": "/wt/d",
                    "cleanup_status": null, "uncleaned": false
                },
                {
                    "task_id": "t_dirty", "handle": "t_dirty", "title": "left over",
                    "status": "in_review", "tab_id": "tab-c", "liveness": "working",
                    "branch": "lane/c", "worktree": "/wt/c",
                    "cleanup_status": "pending", "uncleaned": true
                }
            ]
        })
    }

    #[test]
    fn renders_one_row_per_lane_with_columns() {
        let out = render_lanes(&fixture(), false);
        assert!(out.contains("TASK"));
        assert!(out.contains("t_alive live work"));
        assert!(out.contains("in_progress"));
        assert!(out.contains("alive"));
        assert!(out.contains("lane/a"));
        assert!(out.contains("/wt/a"));
        // dead tab row present in the full table.
        assert!(out.contains("t_dead"));
        assert!(out.contains("dead"));
        // 3 data rows + 1 header.
        assert_eq!(out.lines().count(), 4);
    }

    #[test]
    fn stale_filter_narrows_to_dead_and_uncleaned() {
        let out = render_lanes(&fixture(), true);
        assert!(out.contains("t_dead"));
        assert!(out.contains("t_dirty"));
        assert!(!out.contains("t_alive"));
        // header + 2 stale rows.
        assert_eq!(out.lines().count(), 3);
    }

    #[test]
    fn uncleaned_lane_shows_cleanup_state() {
        let out = render_lanes(&fixture(), true);
        assert!(out.contains("pending"));
    }

    #[test]
    fn empty_lanes_report_gracefully() {
        assert_eq!(
            render_lanes(&json!({"lanes": []}), false),
            "no lanes have been registered\n"
        );
        assert_eq!(
            render_lanes(&json!({"lanes": [], "lane_history_count": 2}), false),
            "all registered lanes are closed\n"
        );
        assert_eq!(render_lanes(&json!({}), true), "no stale lanes\n");
    }

    #[test]
    fn peer_offline_is_not_stale() {
        let val = json!({"lanes": [{
            "task_id": "t_peer", "handle": "t_peer", "title": "remote",
            "status": "in_progress", "liveness": "peer_offline",
            "uncleaned": false
        }]});
        assert!(render_lanes(&val, true).contains("no stale lanes"));
        assert!(render_lanes(&val, false).contains("peer-off"));
    }
}
