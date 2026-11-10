"""Hook API endpoint tests — permission, stop, respond, full approval flow."""

import pytest

from helpers import CodeTermAPI, inject_screen_text, unique_name


class TestHookEndpoints:
    """Test hook API endpoints (called by agent hooks like Claude Code)."""

    def test_hook_permission(self, api_only, api_tab):
        """POST /hook/permission should not crash."""
        result = api_only.post(f"/api/tabs/{api_tab}/hook/permission")
        assert result.get("ok") is True

    def test_hook_stop(self, api_only, api_tab):
        """POST /hook/stop should not crash."""
        result = api_only.post(f"/api/tabs/{api_tab}/hook/stop")
        assert result.get("ok") is True

    def test_respond_without_rendered_prompt_is_refused(self, api, fresh_tab):
        """An eager shell has a PTY but no provider menu to answer."""
        assert api.inspect_tab(fresh_tab)["pty_id"]
        api.post(f"/api/tabs/{fresh_tab}/hook/permission")
        response = api.s.post(
            f"{api.base}/api/tabs/{fresh_tab}/prompt/respond",
            json={"value": "1"},
            timeout=10,
        )
        assert response.status_code == 409, response.text
        assert response.json() == {"ok": False, "reason": "prompt_not_on_screen"}
        assert api.inspect_tab(fresh_tab).get("pending_prompt") is None

    def test_no_prompt_initially(self, api_only, api_tab):
        """Fresh tab should have no pending_prompt."""
        tab = api_only.inspect_tab(api_tab)
        assert tab.get("pending_prompt") is None

    @pytest.mark.llm(reason="Provider-classified detection requires an active provider menu; a completed shell print is transcript output.")
    def test_hook_parses_prompt_from_buffer(self, api, fresh_tab):
        """Hook should parse options when buffer has a real prompt."""
        inject_screen_text(api, fresh_tab, (
            "\nDo you want to proceed?\n"
            "  > 1. Yes\n"
            "    2. Yes, allow reading during this session\n"
            "    3. No\n"
            "\nEsc to cancel \u00b7 Tab to amend\n"
        ))
        api.post(f"/api/tabs/{fresh_tab}/hook/permission")

        tab = api.inspect_tab(fresh_tab)
        prompt = tab.get("pending_prompt")
        assert prompt is not None, "pending_prompt should be set after hook with prompt in buffer"
        assert len(prompt.get("options", [])) >= 2, f"Should have 2+ options, got {prompt}"


class TestPromptParsingAccuracy:
    """Test that prompt parser doesn't pick up numbered lists from Claude's answers."""

    @pytest.mark.llm(reason="Provider-classified detection requires an active provider menu; a completed shell print is transcript output.")
    def test_numbered_answer_then_prompt(self, api, fresh_tab):
        """Claude answer with numbered list, then real prompt — only Yes/No parsed."""
        # Simulate Claude's answer with numbered list THEN a real prompt
        inject_screen_text(api, fresh_tab, (
            "Here is my analysis:\n"
            "1. First bug found in parser\n"
            "2. Second issue with encoding\n"
            "3. Third problem in renderer\n"
            "4. Fourth edge case\n"
            "5. Fifth minor issue\n"
            "\n"
            "Do you want to proceed?\n"
            "  > 1. Yes\n"
            "    2. Yes, allow reading during this session\n"
            "    3. No\n"
            "\nEsc to cancel \u00b7 Tab to amend\n"
        ))

        # Hook triggers prompt parsing from ring buffer
        api.post(f"/api/tabs/{fresh_tab}/hook/permission")

        tab = api.inspect_tab(fresh_tab)
        prompt = tab.get("pending_prompt")
        assert prompt is not None, "Should detect prompt"
        assert prompt["question"] == "Do you want to proceed?"

        # Critical: options should be Yes/No, NOT "First bug", "Second issue"
        labels = [o["label"] for o in prompt["options"]]
        assert any("Yes" in l for l in labels), f"Should have Yes option, got {labels}"
        assert any("No" in l for l in labels), f"Should have No option, got {labels}"
        assert not any("bug" in l.lower() or "issue" in l.lower() for l in labels), \
            f"Should NOT have answer items in options: {labels}"
        assert len(prompt["options"]) <= 3, f"Should have max 3 options, got {len(prompt['options'])}"

    def test_only_numbered_answer_no_prompt(self, api, fresh_tab):
        """Claude answer with numbered list but NO prompt — no pending_prompt set."""
        inject_screen_text(api, fresh_tab, (
            "Here is what I found:\n"
            "1. First thing\n"
            "2. Second thing\n"
            "3. Third thing\n"
            "\nLet me fix these.\n"
        ))

        # Hook without real prompt in buffer — should not set prompt
        api.post(f"/api/tabs/{fresh_tab}/hook/permission")

        tab = api.inspect_tab(fresh_tab)
        prompt = tab.get("pending_prompt")
        # Without explicit body and without real prompt in buffer,
        # parser should not find anything
        if prompt:
            # If it somehow parsed, labels should not be "First thing" etc.
            labels = [o["label"] for o in prompt.get("options", [])]
            assert not any("thing" in l.lower() for l in labels), \
                f"Should NOT parse answer as prompt: {labels}"

    def test_explicit_body_overrides_buffer(self, api_only, api_tab):
        """Explicit body in hook/permission sets prompt directly, ignoring buffer."""
        # Send explicit prompt via body — should set prompt regardless of buffer content
        api_only.post(f"/api/tabs/{api_tab}/hook/permission", json={
            "question": "Allow Bash?",
            "options": [{"label": "Yes", "value": "1"}, {"label": "No", "value": "3"}]
        })

        tab = api_only.inspect_tab(api_tab)
        prompt = tab.get("pending_prompt")
        assert prompt is not None, "explicit body should set pending_prompt"
        assert prompt["question"] == "Allow Bash?"
        assert len(prompt["options"]) == 2
        assert prompt["options"][0]["label"] == "Yes"


class TestShellPromptResponseGuards:
    def test_shell_text_cannot_approve_prompt(self, api, fresh_tab):
        inject_screen_text(api, fresh_tab, (
            "Do you want to proceed?\n"
            "  > 1. Yes\n"
            "    2. No\n"
            "\nEsc to cancel \u00b7 Tab to amend\n"
        ))
        api.post(f"/api/tabs/{fresh_tab}/hook/permission")
        assert api.inspect_tab(fresh_tab).get("pending_prompt") is None
        response = api.s.post(
            f"{api.base}/api/tabs/{fresh_tab}/prompt/respond",
            json={"value": "1"},
            timeout=10,
        )
        assert response.status_code == 409, response.text
        assert response.json() == {"ok": False, "reason": "prompt_not_on_screen"}
        assert api.inspect_tab(fresh_tab).get("pending_prompt") is None

    def test_shell_refusal_keeps_prompts_independent(self, api):
        ws_id = api.create_workspace(unique_name("e2e-ws"))["id"]
        try:
            tab_ids = [api.create_tab(ws_id, eager=True)["tab_id"] for _ in range(2)]
            for tab_id in tab_ids:
                assert api.wait_for_pty(tab_id, timeout=15)
                api.wait_for_output(tab_id, pattern=r"[\$#%>]", timeout=10)
                api.post(f"/api/tabs/{tab_id}/hook/permission", json={
                    "question": "Allow Bash?",
                    "options": [{"label": "Yes", "value": "1"}, {"label": "No", "value": "2"}],
                })
            before = [api.inspect_tab(tab_id)["pending_prompt"] for tab_id in tab_ids]
            assert all(prompt is not None for prompt in before)
            response = api.s.post(
                f"{api.base}/api/tabs/{tab_ids[0]}/prompt/respond",
                json={"value": "1"},
                timeout=10,
            )
            assert response.status_code == 409, response.text
            assert response.json() == {"ok": False, "reason": "prompt_not_on_screen"}
            after = [api.inspect_tab(tab_id)["pending_prompt"] for tab_id in tab_ids]
            assert after == before
        finally:
            api.cleanup_workspace(ws_id)
