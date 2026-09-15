"""Restore report oracle: every agent pane observed live, never assumed."""
import json
import os
import pathlib


def _report_path(data_dir):
    return pathlib.Path(data_dir) / "state" / "recovery" / "last_restore_report.json"


def _load(path, since):
    try:
        if path.stat().st_mtime < since:
            return None
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return None


def assert_settled(args, event_wait, fail):
    deadline, since = int(args[0]), float(args[1])
    rest = args[2:]
    split = rest.index("--") if "--" in rest else len(rest)
    agents, shells = rest[:split], rest[split + 1:]
    data_dir = os.environ.get("CODETERM_DEV_DATA_DIR")
    if not data_dir:
        fail("restore-report must run under dev_run (no CODETERM_DEV_DATA_DIR)")
    path = _report_path(data_dir)
    wait = event_wait(deadline)
    latest = {}

    def settled(_event):
        report = _load(path, since)
        if report is None:
            return False
        latest["report"] = report
        panes = {pane.get("tab_id"): pane for pane in report.get("panes", [])}
        if any(str(panes.get(tab, {}).get("liveness", "")).lower() == "unobserved" for tab in agents):
            return False
        return all(tab in panes for tab in agents)

    wait.until(settled, f"a settled restore report at {path}")
    report = latest["report"]
    panes = {pane.get("tab_id"): pane for pane in report.get("panes", [])}
    failures = []
    for tab in agents:
        pane = panes[tab]
        if not pane.get("restored") or str(pane.get("liveness", "")).lower() != "live":
            failures.append(
                f"agent pane {tab} restored={pane.get('restored')} liveness={pane.get('liveness')} "
                f"drop_reason={pane.get('drop_reason')} agent_type={pane.get('agent_type')} session={pane.get('agent_session_id')}"
            )
    for tab in shells:
        pane = panes.get(tab)
        if pane and pane.get("agent_type"):
            failures.append(f"shell pane {tab} reported with agent_type {pane.get('agent_type')!r}")
    if failures:
        for line in failures:
            print(f"FAIL restore-report {line}")
        print("FAIL restore-report " + json.dumps({k: report.get(k) for k in ("bound", "restored", "dropped", "unobserved", "previous_epoch_end")}))
        raise SystemExit(1)
    print(
        f"PASS restore-report {len(agents)} agent panes live "
        f"(bound={report.get('bound')} restored={report.get('restored')} previous_epoch_end={report.get('previous_epoch_end')})"
    )


def assert_exited(args, event_wait, fail, status):
    """A member that exited to its shell before shutdown restores flagged, not relaunched."""
    deadline, since, tab, session = int(args[0]), float(args[1]), args[2], args[3]
    data_dir = os.environ.get("CODETERM_DEV_DATA_DIR")
    if not data_dir:
        fail("restore-exited must run under dev_run (no CODETERM_DEV_DATA_DIR)")
    path = _report_path(data_dir)
    wait = event_wait(deadline)
    latest = {}

    def observed(_event):
        report = _load(path, since)
        pane = next((p for p in (report or {}).get("panes", []) if p.get("tab_id") == tab), None)
        if pane is None or str(pane.get("liveness", "")).lower() == "unobserved":
            return False
        latest["pane"] = pane
        return True

    wait.until(observed, f"an observed restore verdict for exited member {tab} at {path}")
    pane = latest["pane"]
    failures = []
    if pane.get("restored") or pane.get("drop_reason") != "agent_exited_before_shutdown":
        failures.append(f"pane restored={pane.get('restored')} drop_reason={pane.get('drop_reason')}")
    if pane.get("exited_session_id") != session:
        failures.append(f"pane exited_session_id={pane.get('exited_session_id')!r}, exited session {session!r}")
    record = status(tab) or {}
    if record.get("agent_type") or record.get("agent_session_id"):
        failures.append(f"tab relaunched as {record.get('agent_type')!r}/{record.get('agent_session_id')!r}")
    if not record.get("role") or not record.get("team_id"):
        failures.append(f"tab lost role/team: role={record.get('role')!r} team={record.get('team_id')!r}")
    if failures:
        for line in failures:
            print(f"FAIL restore-exited {tab} {line}")
        raise SystemExit(1)
    print(f"PASS restore-exited {tab} flagged with session {session}, not relaunched")
