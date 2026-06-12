"""Pane management tests: list, rename, focus."""

from helpers import CodeTermAPI, unique_name


def test_list_panes(api_only: CodeTermAPI):
    """At least one pane exists (from the default workspace)."""
    panes = api_only.list_panes()
    assert isinstance(panes, list)
    assert len(panes) >= 1


def test_rename_pane(api_only: CodeTermAPI, api_pane: str):
    """Rename a pane and verify in the pane list."""
    new_title = unique_name("renamed")
    result = api_only.rename_pane(api_pane, new_title)
    assert result.get("ok") is True

    panes = api_only.list_panes()
    found = [p for p in panes if p["id"] == api_pane]
    assert len(found) == 1
    assert found[0]["title"] == new_title


def test_focus_pane(api_only: CodeTermAPI, api_pane: str):
    """Focus a pane (should not error)."""
    result = api_only.focus_pane(api_pane)
    assert result.get("ok") is True
