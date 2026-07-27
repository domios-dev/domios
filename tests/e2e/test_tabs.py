"""Tab management tests: list, rename, focus."""

from helpers import CodeTermAPI, unique_name


def test_list_tabs(api_only: CodeTermAPI):
    """At least one tab exists (from the default workspace)."""
    tabs = api_only.list_tabs()
    assert isinstance(tabs, list)
    assert len(tabs) >= 1


def test_rename_tab(api_only: CodeTermAPI, api_tab: str):
    """Rename a tab and verify in the tab list."""
    new_title = unique_name("renamed")
    result = api_only.rename_tab(api_tab, new_title)
    assert result.get("ok") is True

    tabs = api_only.list_tabs()
    found = [p for p in tabs if p["id"] == api_tab]
    assert len(found) == 1
    assert found[0]["title"] == new_title


def test_focus_tab(api_only: CodeTermAPI, api_tab: str):
    """Focus a tab (should not error)."""
    result = api_only.focus_tab(api_tab)
    assert result.get("ok") is True
