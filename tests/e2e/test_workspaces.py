"""Workspace CRUD tests."""

import os

from helpers import CodeTermAPI, unique_name


def test_create_workspace(api_only: CodeTermAPI):
    """Create a workspace and verify it appears in the list."""
    name = unique_name("ws-create")
    ws = api_only.create_workspace(name)
    assert "id" in ws

    workspaces = api_only.list_workspaces()
    ids = [w["id"] for w in workspaces]
    assert ws["id"] in ids

    # Cleanup
    api_only.close_workspace(ws["id"])


def test_rename_workspace(api_only: CodeTermAPI):
    """Create, rename, verify new name in list."""
    name = unique_name("ws-rename")
    ws = api_only.create_workspace(name)
    ws_id = ws["id"]

    new_name = unique_name("ws-renamed")
    result = api_only.rename_workspace(ws_id, new_name)
    assert result.get("ok") is True

    workspaces = api_only.list_workspaces()
    found = [w for w in workspaces if w["id"] == ws_id]
    assert len(found) == 1
    assert found[0]["name"] == new_name

    api_only.close_workspace(ws_id)


def test_close_workspace(api_only: CodeTermAPI):
    """Close a workspace and verify tabs are gone."""
    name = unique_name("ws-close")
    ws = api_only.create_workspace(name)
    ws_id = ws["id"]

    # Grab tab IDs belonging to this workspace before closing
    tabs_before = api_only.list_tabs()
    # There should be at least one tab from the new workspace

    result = api_only.close_workspace(ws_id)
    assert result.get("ok") is True

    # Workspace should no longer appear
    workspaces = api_only.list_workspaces()
    ids = [w["id"] for w in workspaces]
    assert ws_id not in ids


def test_create_tab_in_workspace(api_only: CodeTermAPI):
    """Create a new tab in a workspace, verify it returns a tab_id."""
    name = unique_name("ws-tab")
    ws = api_only.create_workspace(name)
    ws_id = ws["id"]

    tab = api_only.create_tab(ws_id)
    assert "tab_id" in tab
    assert "tab_id" in tab
    assert len(tab["tab_id"]) == 8  # 8-char hex

    api_only.close_workspace(ws_id)


def test_create_workspace_cwd_dedupe(api_only: CodeTermAPI):
    """Second create with the same --cwd returns existing=true and no new tab."""
    cwd = os.path.normpath(os.getcwd())
    name = unique_name("ws-cwd")
    ws1 = api_only.create_workspace(name, cwd=cwd)
    assert "id" in ws1
    assert ws1.get("existing") is None  # first call: not a dedupe

    ws2 = api_only.create_workspace(unique_name("ws-cwd2"), cwd=cwd)
    assert ws2["id"] == ws1["id"], "dedupe must return the original workspace"
    assert ws2.get("existing") is True, "dedupe response must have existing=true"

    # No extra tab should have been created
    workspaces = api_only.list_workspaces()
    found = next((w for w in workspaces if w["id"] == ws1["id"]), None)
    assert found is not None
    assert found["tab_count"] == ws1.get("tab_count", 0), "tab count must not change on dedupe"

    api_only.close_workspace(ws1["id"])


def test_create_workspace_fresh_counters(api_only: CodeTermAPI):
    """Freshly created workspace has consistent tab_count and tab_ids."""
    name = unique_name("ws-counters")
    ws = api_only.create_workspace(name)
    assert ws.get("tab_count", -1) == 0, "new workspace must start with tab_count=0"
    assert ws.get("tab_ids") == [], "new workspace must start with empty tab_ids"
    assert ws.get("existing") is None

    api_only.close_workspace(ws["id"])


def test_create_tab_follows_creator_not_focus(api_only: CodeTermAPI):
    """POST /api/tabs lands in the creator tab's workspace, not the focused one.

    Agents kept creating their shells in whatever workspace the user had
    selected; the creator tab is the identity that decides.
    """
    home = api_only.create_workspace(unique_name("ws-creator-home"))
    elsewhere = api_only.create_workspace(unique_name("ws-creator-focused"))
    try:
        creator = api_only.create_tab(home["id"])
        api_only.post(f"/api/workspaces/{elsewhere['id']}/focus")

        created = api_only.post(
            "/api/tabs", json={"creator_tab_id": creator["tab_id"]}
        )
        assert created["workspace_id"] == home["id"], (
            "tab must inherit the creator's workspace, "
            f"got {created['workspace_id']}"
        )

        explicit = api_only.post(
            "/api/tabs",
            json={
                "creator_tab_id": creator["tab_id"],
                "workspace_id": elsewhere["id"],
            },
        )
        assert explicit["workspace_id"] == elsewhere["id"], (
            "an explicit workspace_id still wins over the creator"
        )
    finally:
        api_only.close_workspace(home["id"])
        api_only.close_workspace(elsewhere["id"])
