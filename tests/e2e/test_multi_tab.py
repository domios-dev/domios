"""Multi-tab prompt isolation tests."""

import time
import pytest
from helpers import CodeTermAPI, inject_screen_text, unique_name


@pytest.fixture()
def three_tabs(api: CodeTermAPI):
    """Create workspace with 3 tabs, yield (ws_id, [tab_ids]), cleanup."""
    ws = api.create_workspace(unique_name("e2e-multi"))
    ws_id = ws["id"]
    tab_ids = []
    for _ in range(3):
        tab = api.create_tab(ws_id, eager=True)
        pid = tab["tab_id"]
        assert api.wait_for_pty(pid, timeout=15)
        tab_ids.append(pid)
    for pid in tab_ids:
        try:
            api.wait_for_output(pid, pattern=r"[\$#%>]", timeout=10)
        except TimeoutError:
            time.sleep(2)
    yield ws_id, tab_ids
    try:
        if len(api.list_workspaces()) > 1:
            api.close_workspace(ws_id)
    except Exception:
        pass


class TestMultiTabPrompts:
    """Test independent prompts across multiple tabs."""

    def _set_prompt(self, api, tab_id):
        inject_screen_text(api, tab_id, (
            "Do you want to proceed?\n"
            "  > 1. Yes\n"
            "    2. No\n"
            "\nEsc to cancel \u00b7 Tab to amend\n"
        ))
        time.sleep(0.5)
        api.post(f"/api/tabs/{tab_id}/hook/permission")

    def _tab(self, api, tab_id):
        return next(p for p in api.list_tabs() if p["id"] == tab_id)

    def test_two_tabs_independent(self, api, three_tabs):
        """Two tabs can have prompts simultaneously."""
        _, tabs = three_tabs
        self._set_prompt(api, tabs[0])
        self._set_prompt(api, tabs[1])
        time.sleep(0.5)
        tab_a = self._tab(api, tabs[0])
        tab_b = self._tab(api, tabs[1])
        assert tab_a.get("pending_prompt") is not None
        assert tab_b.get("pending_prompt") is not None

    def test_respond_one_doesnt_clear_other(self, api, three_tabs):
        """Responding in tab A should not affect tab B."""
        _, tabs = three_tabs
        self._set_prompt(api, tabs[0])
        self._set_prompt(api, tabs[1])
        time.sleep(0.5)
        api.post(f"/api/tabs/{tabs[0]}/prompt/respond", json={"value": "1"})
        time.sleep(0.5)
        tab_list = api.list_tabs()
        a = next(p for p in tab_list if p["id"] == tabs[0])
        b = next(p for p in tab_list if p["id"] == tabs[1])
        assert a.get("pending_prompt") is None
        assert b.get("pending_prompt") is not None

    def test_uninvolved_tab_clean(self, api, three_tabs):
        """Tab without hook call should have no prompt."""
        _, tabs = three_tabs
        api.post(f"/api/tabs/{tabs[0]}/hook/permission")
        time.sleep(0.5)
        c = next(p for p in api.list_tabs() if p["id"] == tabs[2])
        assert c.get("pending_prompt") is None

    def test_all_tabs_unique_ids(self, api, three_tabs):
        """All tabs should have unique IDs."""
        _, tab_ids = three_tabs
        assert len(set(tab_ids)) == 3

    def test_all_tabs_are_orchestrators(self, api, three_tabs):
        """All API-created tabs should be Orchestrators."""
        _, tab_ids = three_tabs
        tab_list = api.list_tabs()
        for pid in tab_ids:
            tab = next(p for p in tab_list if p["id"] == pid)
            assert tab.get("role") == "Orchestrator"
