"""Chat-capable daemon-spawned agents default to Chat and accept explicit view choices."""

from helpers import CodeTermAPI, unique_name


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


def test_cli_spawned_agent_defaults_to_chat_view(api_only: CodeTermAPI):
    ws = api_only.create_workspace(unique_name("e2e-ws-chat-default"))
    tab_id = None
    try:
        tab_id = _spawn_agent(api_only, ws["id"])
        assert _tab(api_only, tab_id).get("chat_view") is True
    finally:
        if tab_id:
            api_only.close_tab(tab_id)
        if len(api_only.list_workspaces()) > 1:
            api_only.close_workspace(ws["id"])


def test_explicit_chat_view_choice_updates_same_agent_tab(api_only: CodeTermAPI):
    ws = api_only.create_workspace(unique_name("e2e-ws-chat-choice"))
    tab_id = None
    try:
        tab_id = _spawn_agent(api_only, ws["id"])
        for enabled in (False, True):
            result = api_only.post(
                f"/api/tabs/{tab_id}/chat-view",
                json={"enabled": enabled},
            )
            assert result.get("ok") is True
            assert _tab(api_only, tab_id).get("chat_view") is enabled
    finally:
        if tab_id:
            api_only.close_tab(tab_id)
        if len(api_only.list_workspaces()) > 1:
            api_only.close_workspace(ws["id"])
