"""Chat UI default config applies to daemon-spawned agents."""

from pathlib import Path
import os
import subprocess

from helpers import CodeTermAPI, IS_WINDOWS, unique_name


PROJECT_ROOT = Path(__file__).resolve().parents[2]
GIT_BASH = r"C:\Program Files\Git\bin\bash.exe"


def _set_chat_default(api: CodeTermAPI, enabled: bool) -> None:
    response = api.s.patch(
        f"{api.base}/api/config",
        json={"key": "ui.default_new_agent_tab_chat", "value": enabled},
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
    tab_id = result["tab_id"]
    assert tab_id
    return tab_id


def _tab(api: CodeTermAPI, tab_id: str) -> dict:
    tab = next((p for p in api.list_tabs() if p["id"] == tab_id), None)
    assert tab is not None
    return tab


def _restart_dev(api: CodeTermAPI) -> None:
    assert api.base == "http://127.0.0.1:7686"
    before = api.get("/api/health", timeout=10)
    build_sha = before.get("build_sha")
    assert build_sha, before
    assert before.get("status") == "ok", before
    assert before.get("startup", {}).get("state") == "ready", before

    bash = GIT_BASH if IS_WINDOWS else "bash"
    subprocess.run(
        [bash, "build.sh", "dev-prebuilt", "--wait"],
        cwd=PROJECT_ROOT,
        env={**os.environ, "CODETERM_DEV_INSTANCE": "", "CODETERM_DEV_API_PORT": "7686"},
        check=True,
        timeout=180,
    )

    after = api.get("/api/health", timeout=10)
    assert after.get("status") == "ok", after
    assert after.get("startup", {}).get("state") == "ready", after
    assert after.get("build_sha") == build_sha, (before, after)


def test_chat_ui_default_on_sets_cli_spawned_agent_chat_view(api_only: CodeTermAPI):
    ws = api_only.create_workspace(unique_name("e2e-ws-chat-default-on"))
    tab_id = None
    try:
        _set_chat_default(api_only, True)
        tab_id = _spawn_agent(api_only, ws["id"])
        assert _tab(api_only, tab_id).get("chat_view") is True
    finally:
        _set_chat_default(api_only, False)
        if tab_id:
            api_only.close_tab(tab_id)
        if len(api_only.list_workspaces()) > 1:
            api_only.close_workspace(ws["id"])


def test_chat_ui_default_restores_on_same_agent_tab(api_only: CodeTermAPI):
    ws = api_only.create_workspace(unique_name("e2e-ws-chat-default-restore"))
    tab_id = None
    try:
        _set_chat_default(api_only, True)
        tab_id = _spawn_agent(api_only, ws["id"])
        assert _tab(api_only, tab_id).get("chat_view") is True
        api_only.post("/api/session/save")

        _restart_dev(api_only)

        restored = _tab(api_only, tab_id)
        assert restored.get("chat_view") is True
    finally:
        _set_chat_default(api_only, False)
        if tab_id:
            try:
                api_only.close_tab(tab_id)
            except Exception:
                pass
        try:
            if len(api_only.list_workspaces()) > 1:
                api_only.close_workspace(ws["id"])
        except Exception:
            pass


def test_chat_ui_default_off_keeps_cli_spawned_agent_terminal(api_only: CodeTermAPI):
    ws = api_only.create_workspace(unique_name("e2e-ws-chat-default-off"))
    tab_id = None
    try:
        _set_chat_default(api_only, False)
        tab_id = _spawn_agent(api_only, ws["id"])
        assert _tab(api_only, tab_id).get("chat_view") is False
    finally:
        if tab_id:
            api_only.close_tab(tab_id)
        if len(api_only.list_workspaces()) > 1:
            api_only.close_workspace(ws["id"])
