"""Chat UI default config applies to daemon-spawned agents."""

from pathlib import Path
import subprocess
import time

from helpers import CodeTermAPI, IS_WINDOWS, unique_name


PROJECT_ROOT = Path(__file__).resolve().parents[2]
GIT_BASH = r"C:\Program Files\Git\bin\bash.exe"


def _set_chat_default(api: CodeTermAPI, enabled: bool) -> None:
    response = api.s.patch(
        f"{api.base}/api/config",
        json={"key": "ui.default_new_agent_pane_chat", "value": enabled},
    )
    response.raise_for_status()
    result = response.json()
    assert result.get("ok") is True


def _spawn_agent(api: CodeTermAPI, workspace_id: str) -> str:
    result = api.post(
        "/api/agents/spawn",
        json={
            "agent": "claude",
            "workspace_id": workspace_id,
            "skip_permissions": True,
        },
    )
    pane_id = result["pane_id"]
    assert pane_id
    return pane_id


def _pane(api: CodeTermAPI, pane_id: str) -> dict:
    pane = next((p for p in api.list_panes() if p["id"] == pane_id), None)
    assert pane is not None
    return pane


def _restart_dev() -> None:
    bash = GIT_BASH if IS_WINDOWS else "bash"
    subprocess.run([bash, "build.sh", "kill"], cwd=PROJECT_ROOT, check=True, timeout=60)
    subprocess.run([bash, "build.sh", "dev", "--wait"], cwd=PROJECT_ROOT, check=True, timeout=180)


def _wait_for_pane(api: CodeTermAPI, pane_id: str, timeout: float = 30) -> dict:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        pane = next((p for p in api.list_panes() if p["id"] == pane_id), None)
        if pane:
            return pane
        time.sleep(0.5)
    raise AssertionError(f"pane {pane_id} did not restore within {timeout}s")


def test_chat_ui_default_on_sets_cli_spawned_agent_chat_view(api_only: CodeTermAPI):
    ws = api_only.create_workspace(unique_name("e2e-ws-chat-default-on"))
    pane_id = None
    try:
        _set_chat_default(api_only, True)
        pane_id = _spawn_agent(api_only, ws["id"])
        assert _pane(api_only, pane_id).get("chat_view") is True
    finally:
        _set_chat_default(api_only, False)
        if pane_id:
            api_only.close_pane(pane_id)
        if len(api_only.list_workspaces()) > 1:
            api_only.close_workspace(ws["id"])


def test_chat_ui_default_restores_on_same_agent_pane(api_only: CodeTermAPI):
    ws = api_only.create_workspace(unique_name("e2e-ws-chat-default-restore"))
    pane_id = None
    try:
        _set_chat_default(api_only, True)
        pane_id = _spawn_agent(api_only, ws["id"])
        assert _pane(api_only, pane_id).get("chat_view") is True
        api_only.post("/api/session/save")

        _restart_dev()

        restored = _wait_for_pane(api_only, pane_id)
        assert restored.get("chat_view") is True
    finally:
        _set_chat_default(api_only, False)
        if pane_id:
            try:
                api_only.close_pane(pane_id)
            except Exception:
                pass
        try:
            if len(api_only.list_workspaces()) > 1:
                api_only.close_workspace(ws["id"])
        except Exception:
            pass


def test_chat_ui_default_off_keeps_cli_spawned_agent_terminal(api_only: CodeTermAPI):
    ws = api_only.create_workspace(unique_name("e2e-ws-chat-default-off"))
    pane_id = None
    try:
        _set_chat_default(api_only, False)
        pane_id = _spawn_agent(api_only, ws["id"])
        assert _pane(api_only, pane_id).get("chat_view") is False
    finally:
        if pane_id:
            api_only.close_pane(pane_id)
        if len(api_only.list_workspaces()) > 1:
            api_only.close_workspace(ws["id"])
