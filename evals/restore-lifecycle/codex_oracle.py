"""Fail-closed evidence checks for the Codex restart matrix."""
import json
import re

GUI_PATH = "/usr/bin:/bin:/usr/sbin:/sbin"
CONTINUE = "Domios restarted while you were working. Continue the work you were doing."
IDENTITY = ("agent_type", "agent_session_id", "role", "role_profile", "team_id", "parent_tab_id", "title")


def require(condition, detail):
    if not condition:
        raise AssertionError(detail)


def cli_data(payload):
    if isinstance(payload, dict) and 'kind' in payload and 'ok' in payload:
        require(payload['ok'] is True, f'CLI refused: {payload}')
        require('data' in payload, f'CLI envelope has no data: {payload}')
        return payload['data']
    return payload


def inspected(payload):
    require(isinstance(payload, list) and len(payload) == 1, "inspect must return exactly one tab")
    return payload[0]


def bindings(records, before=None, provider="codex"):
    sessions = set()
    for tab, row in records.items():
        sid, nonce = row.get("agent_session_id"), row.get("session_binding_nonce")
        require(row.get("agent_type") == provider and sid, f"{tab}: missing {provider} session")
        require(sid not in sessions, f"{tab}: duplicate session {sid}")
        sessions.add(sid)
        require(nonce and nonce == row.get("reported_session_binding_nonce"), f"{tab}: unauthenticated nonce")
        require(row.get("boot_stage") == "agent_ready", f"{tab}: boot_stage={row.get('boot_stage')}")
        require(not row.get("boot_failure_reason"), f"{tab}: {row.get('boot_failure_reason')}")
        require("structured_control_active" in row, f"{tab}: missing structured control telemetry")
        if before:
            for key in IDENTITY:
                require(row.get(key) == before[tab].get(key), f"{tab}: {key} changed")


def user_texts(path):
    result = []
    with open(path, encoding="utf-8") as stream:
        for raw in stream:
            try:
                row = json.loads(raw)
            except json.JSONDecodeError:
                continue
            payload = row.get("payload", {})
            if row.get("type") == "response_item" and payload.get("type") == "message" and payload.get("role") == "user":
                result.append("\n".join(part.get("text", "") for part in payload.get("content", [])))
    return result


def assistant_texts(path):
    result = []
    with open(path, encoding="utf-8") as stream:
        for raw in stream:
            try:
                row = json.loads(raw)
            except json.JSONDecodeError:
                continue
            payload = row.get("payload", {})
            if row.get("type") == "response_item" and payload.get("type") == "message" and payload.get("role") == "assistant":
                result.append("\n".join(part.get("text", "") for part in payload.get("content", [])))
    return result


def exact_markers(transcripts, markers):
    for tab, marker in markers.items():
        for owner, texts in transcripts.items():
            count = sum(marker in text for text in texts)
            require(count == int(owner == tab), f"{marker}: {owner} own-session user count={count}")


def restart_logs(log, tabs, idle=False):
    failures = [line for line in log.splitlines() if any(word in line for word in
                ("expired_launch", "refused_pending", "SessionBindingNotObserved", "STALE TASK BINDING", "launch failed"))]
    require(not failures, "\n".join(failures))
    for tab in tabs:
        delivered = [line for line in log.splitlines() if "[turn-resume]" in line and
                     f"tab={tab} " in line and "outcome=delivered" in line]
        require(len(delivered) == (0 if idle else 1), f"{tab}: delivered continue outcomes={len(delivered)}\n" + "\n".join(delivered))


def daemon_environment(raw, mode):
    paths = re.findall(r"(?:^|\s)PATH=([^\s]*)", raw)
    require(len(paths) == 1, "daemon environment has no unique PATH")
    if mode == "gui":
        require(paths[0] == GUI_PATH, f"daemon PATH={paths[0]!r}, expected {GUI_PATH!r}")
    require(re.search(r"(?:^|\s)CODETERM_INSTANCE=restartqa(?:\s|$)", raw), "daemon instance is not restartqa")
    require(re.search(r"(?:^|\s)CODETERM_API_PORT=7692(?:\s|$)", raw), "daemon port is not 7692")


def current_attempt(detail, tab):
    task = detail.get("task", detail)
    attempts = task.get("execution_attempts", [])
    matches = [a for a in attempts if a.get("executor_tab_id") == tab and not a.get("closed_at")]
    require(len(matches) == 1, f"{tab}: no unique open attempt: {attempts}")
    require(matches[0].get("acknowledged_at") is not None, f"{tab}: Worker ACK missing")
    return matches[0]


def tool_outputs(path):
    result = []
    with open(path, encoding="utf-8") as stream:
        for raw in stream:
            try:
                row = json.loads(raw)
            except json.JSONDecodeError:
                continue
            payload = row.get("payload", {})
            if row.get("type") == "response_item" and payload.get("type") in ("function_call_output", "custom_tool_call_output"):
                result.append(payload.get("output", ""))
    return result


def resumed_numeric_output(path, baseline_user_count):
    users = 0
    continued = False
    with open(path, encoding="utf-8") as stream:
        for raw in stream:
            try:
                row = json.loads(raw)
            except json.JSONDecodeError:
                continue
            payload = row.get("payload", {})
            if row.get("type") != "response_item" or payload.get("type") != "message":
                continue
            text = "\n".join(part.get("text", "") for part in payload.get("content", []))
            if payload.get("role") == "user":
                users += 1
                if users > baseline_user_count and CONTINUE in text:
                    continued = True
            if continued and payload.get("role") == "assistant" and any(line.strip().isdigit() for line in text.splitlines()):
                return True
    return False
