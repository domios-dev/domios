"""Agent hierarchy tests — roles and reports without LLM providers."""

from helpers import CodeTermAPI, shell_cmd, unique_name


def create_api_tab(api: CodeTermAPI, title: str) -> str:
    result = api.post("/api/tabs", json={"title": title, "eager": False})
    tab_id = result["tab_id"]
    assert tab_id
    return tab_id


def test_create_api_tab(api_only: CodeTermAPI):
    """Create a lazy tab and verify it appears with the requested title."""
    title = unique_name("agent-shell")
    tab_id = create_api_tab(api_only, title)
    try:
        tab = next((p for p in api_only.list_tabs() if p["id"] == tab_id), None)
        assert tab is not None
        assert tab.get("title") == title
    finally:
        api_only.close_tab(tab_id)


def test_set_role(api_only: CodeTermAPI, api_tab: str):
    """Set Orchestrator role on a tab."""
    result = api_only.post(
        f"/api/tabs/{api_tab}/role",
        json={"role": "Orchestrator"},
    )
    assert result.get("ok") is True

    # Verify via status
    status = api_only.get(f"/api/tabs/{api_tab}/status")
    assert status.get("role") == "Orchestrator"


def test_hierarchy(api_only: CodeTermAPI, api_tab: str):
    """Set orchestrator, spawn worker, verify hierarchy tree."""
    api_only.post(
        f"/api/tabs/{api_tab}/role",
        json={"role": "Orchestrator"},
    )

    worker_id = create_api_tab(api_only, unique_name("worker"))
    try:
        api_only.post(
            f"/api/tabs/{worker_id}/role",
            json={"role": "Worker", "parent_tab_id": api_tab},
        )

        # Hierarchy is derived from parent_tab_id now — the /api/agents/hierarchy
        # and /api/tabs/{id}/children endpoints were removed (tree is reconstructed
        # client-side from the flat tab list).
        tabs = api_only.list_tabs()
        worker_tab = next((p for p in tabs if p["id"] == worker_id), None)
        assert worker_tab is not None
        assert worker_tab.get("parent_tab_id") == api_tab
    finally:
        api_only.close_tab(worker_id)


def test_submit_report(api_only: CodeTermAPI, api_tab: str):
    """Worker reports to parent, verify in reports endpoint."""
    api_only.post(
        f"/api/tabs/{api_tab}/role",
        json={"role": "Orchestrator"},
    )

    worker_id = create_api_tab(api_only, unique_name("reporter"))
    try:
        api_only.post(
            f"/api/tabs/{worker_id}/role",
            json={"role": "Worker", "parent_tab_id": api_tab},
        )

        # Submit report from worker
        report_msg = unique_name("report-msg")
        result = api_only.post(
            f"/api/tabs/{worker_id}/report",
            json={"message": report_msg},
        )
        assert result.get("ok") is True
        assert "report_id" in result

        # Verify report appears in parent's reports
        reports = api_only.get(f"/api/tabs/{api_tab}/reports")
        assert isinstance(reports, list)
        report_messages = [r.get("message", "") for r in reports]
        assert any(report_msg in m for m in report_messages)
    finally:
        api_only.close_tab(worker_id)


def test_shell_tab_accepts_sent_task_text(api: CodeTermAPI, shared_eager_tab: str):
    """Send task text to a shell tab and verify the command executes."""
    task_marker = f"TASK_{unique_name('t')}"
    split = len(task_marker) // 2
    left, right = task_marker[:split], task_marker[split:]
    api.send_text(
        shared_eager_tab,
        text=shell_cmd(f"echo '{left}''{right}'", f"Write-Output ('{left}' + '{right}')"),
    )
    output = api.wait_for_output(shared_eager_tab, pattern=task_marker, timeout=15)
    assert task_marker in output
