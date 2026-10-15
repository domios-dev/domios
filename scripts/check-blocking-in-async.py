#!/usr/bin/env python3
"""Reject ordered state locks on async runtime threads."""
# Caller: scripts/check-blocking-in-async.sh, scripts/tests/test_check_blocking_in_async.py
import re
import subprocess
from pathlib import Path

TOKEN = re.compile(
    r'/\*.*?\*/|//[^\n]*|r(?P<hash>\#*)".*?"(?P=hash)|"(?:\\.|[^"\\])*"|'
    r"'(?:\\.|[^'\\])'|[A-Za-z_][A-Za-z_0-9]*|[^\s]",
    re.DOTALL,
)
LOCKS = {"mux": {"read", "write"}, **{name: {"lock"} for name in
         ("pty", "screen_state", "agent_tracker", "output_buf")}}
LOCKING_HELPERS = {
    "resolve_tab", "resolve_hook_tab", "resolve_pty_id_with_fallback",
    "tab_shell_exited", "resolve_target_pty", "resolve_routed_tab",
    "surface_send_refusal", "bind_reported_session",
    "deferred_binding_still_applies", "reject_foreign_binding_nonce",
    "ensure_agent_state", "collect_owned_pty_tabs", "resolve_pty_id",
    "tab_has_agent", "tab_has_agent_identity", "tab_chat_backing",
    "require_managed_send_allowed", "dispatch", "after_response", "clear_prompt",
    "session_binding_state", "tab_probe", "focus_tab", "set_active_tab",
    "write_tab_agent_type_with_origin", "set_tab_agent_type",
    "cancel_staged_model_switch", "arm_bound_worker_ack_deadline_for_tab",
    "make_orchestrator", "transition_plan_planner", "get_tab_status",
    "submit_report_with_source_outcome", "set_can_spawn",
    "permission_auto_confirm_state", "run_tab_menu_item", "set_receiver_target",
    "request_turn_resume_for_caller", "suppress_turn_resume_for_caller",
    "withdraw_turn_resume_request", "list_surfaces", "open_surface", "close_surface",
    "set_chat_view", "move_tab", "reorder_tab", "reorder_child", "reorder_display",
    "reorder_workspace", "validate_active_team_plan_bindings", "validate_plan_team_binding",
    "unarchive_team_plan", "pause_plan_operation", "plan_pause_preview", "plan_resume_preview",
    "resume_plan_operation", "transfer_team_plan", "resolved_worker_launch_effort_for_tab",
    "ensure_tab_pty_for_send", "run_outbound_stages", "substitute_paste_limited_body",
    "write_raw", "deliver_with_pad_file", "deliver_agent_payload", "bridge_is_general_tab",
    "rename_team", "update_tab_cwd", "verify_task_owner_session", "emit_session_binding_state",
    "start_chat_send", "bridge_action", "bridge_target", "attributed_body",
    "drain_tab_output", "clear_tab_role", "chat_read_target",

    "set_and_lock_tab_agent_type_from", "rename_tab", "link_pty",
    "identify_caller", "authorize_peer_request", "authorize_request",

}


def findings(source, check_locks=True, check_auth_locks=False):
    tokens = [(m.group(), m.start()) for m in TOKEN.finditer(source)
              if not m.group().startswith(("//", "/*", '"'))
              and not re.match(r'^r\#*"', m.group())]
    words = [word for word, _ in tokens]
    pairs = {}
    stack = []
    for i, word in enumerate(words):
        if word in ("{", "(", "["):
            stack.append(i)
        elif word in ("}", ")", "]") and stack:
            opening = stack.pop()
            pairs[opening] = i

    def body_after(i):
        while i < len(words) and words[i] not in ("{", ";"):
            i += 1
        return (i, pairs.get(i, i))

    async_bodies = [body_after(i + 2) for i in range(len(words) - 1)
                    if words[i:i + 2] == ["async", "fn"]]
    async_bodies += [(i + 1, pairs.get(i + 1, i + 1)) for i, word in enumerate(words[:-1])
                     if word == "async" and words[i + 1] == "{"]
    async_bodies += [(i + 2, pairs.get(i + 2, i + 2)) for i in range(len(words) - 2)
                     if words[i:i + 3] == ["async", "move", "{"]]
    async_bodies += [(i + 2, pairs.get(i + 2, i + 2)) for i in range(3, len(words) - 2)
                     if words[i:i + 3] == ["stream", "!", "{"]
                     and words[i - 3:i] == ["async_stream", ":", ":"]]
    blocking = []
    for i, word in enumerate(words[:-1]):
        if word not in ("spawn_blocking", "blocking_run", "on_blocking_pool",
                        "off_runtime", "run_programmatic_delivery") or words[i + 1] != "(":
            continue
        end = pairs.get(i + 1, i + 1)
        for j in range(i + 2, end - 1):
            if words[j] == "|":
                second = j + 1
                while second < end and words[second] != "|":
                    second += 1
                if second < end:
                    blocking.append((second, end))
                break
    test_bodies = []
    for i in range(len(words) - 6):
        if words[i:i + 7] == ["#", "[", "cfg", "(", "test", ")", "]"]:
            test_bodies.append(body_after(i + 7))
    result = []
    for i in range(1, len(words) - 1):
        lock = None
        if (check_locks and words[i] in LOCKS
                and words[i + 1:i + 2] == ["."]
                and words[i + 2] in LOCKS[words[i]]
                and words[i + 3:i + 5] == ["(", ")"]):
            lock = f".{words[i]}.{words[i + 2]}()"
        elif (check_locks and words[i] in LOCKING_HELPERS and words[i + 1] == "("
              and words[i - 1] != "fn"):
            lock = f"{words[i]}()"
        if (check_auth_locks and words[i] in ("config", "mesh")
                and words[i + 1:i + 5] == [".", "lock", "(", ")"]):
            lock = f".{words[i]}.lock()"
        rest = "".join(words[i:i + 12])
        for pattern in ("std::process::Command::new", "std::thread::sleep",
                        "reqwest::blocking::"):
            if rest.startswith(pattern):
                lock = pattern.rstrip(":")
        if (words[i:i + 5] == ["Command", ":", ":", "new", "("]
                and words[i - 1] != ":"
                and re.search(r"use\s+std::process::Command\b", source)):
            lock = "Command::new"
        if lock is None:
            continue
        call_end = pairs.get(i + 1)
        if call_end is not None and words[call_end + 1:call_end + 3] == [".", "await"]:
            continue
        if not any(start < i < end for start, end in async_bodies):
            continue
        if any(start < i < end for start, end in blocking + test_bodies):
            continue
        line = source.count("\n", 0, tokens[i][1]) + 1
        result.append((line, lock))
    return result


def main(repo=None):
    if repo is None:
        repo = Path(subprocess.check_output(["git", "rev-parse", "--show-toplevel"],
                                           text=True).strip())
    crates = repo / "src-tauri/crates"
    roots = [crates / name / "src" for name in
             ("codeterm-api", "codeterm-commands", "codeterm-socket",
              "codeterm-sentinel-host")]
    roots += [crates / "codeterm-service-core/src" / name
              for name in ("api_support", "startup")]
    roots.append(crates / "codeterm-service-core/src/service")
    bad = []
    for root in roots:
        for path in sorted(root.rglob("*.rs")):
            if (any("tests" in part for part in path.relative_to(root).parts)
                    or path.stem.endswith("_test")):
                continue
            bad.extend(f"{path.relative_to(repo).as_posix()}:{line}:{lock}"
                       for line, lock in findings(path.read_text(encoding="utf-8"),
                                                   check_locks=root.name != "service",
                                                   check_auth_locks=path.name == "auth.rs"))
    allowlist = repo / "scripts/blocking-in-async-allowlist.txt"
    allowed = {line.strip() for line in allowlist.read_text(encoding="utf-8").splitlines()
               if line.strip() and not line.lstrip().startswith("#")} if allowlist.exists() else set()
    bad = [finding for finding in bad if finding not in allowed]
    if bad:
        print("\n".join(bad))
        return 1
    print("check-blocking-in-async: no unallowlisted blocking calls or ordered state locks")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
