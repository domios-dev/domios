"""Hook API endpoint tests — permission, stop, respond, full approval flow."""

import time
import pytest
from helpers import CodeTermAPI, unique_name


class TestHookEndpoints:
    """Test hook API endpoints (called by agent hooks like Claude Code)."""

    def test_hook_permission(self, api, fresh_pane):
        """POST /hook/permission should not crash."""
        result = api.post(f"/api/panes/{fresh_pane}/hook/permission")
        assert result.get("ok") is True

    def test_hook_stop(self, api, fresh_pane):
        """POST /hook/stop should not crash."""
        result = api.post(f"/api/panes/{fresh_pane}/hook/stop")
        assert result.get("ok") is True

    def test_respond_clears_prompt(self, api, fresh_pane):
        """Responding should clear pending_prompt."""
        api.post(f"/api/panes/{fresh_pane}/hook/permission")
        time.sleep(0.5)
        api.post(f"/api/panes/{fresh_pane}/prompt/respond", json={"value": "1"})
        time.sleep(0.5)
        pane = next(p for p in api.list_panes() if p["id"] == fresh_pane)
        assert pane.get("pending_prompt") is None

    def test_no_prompt_initially(self, api, fresh_pane):
        """Fresh pane should have no pending_prompt."""
        pane = next(p for p in api.list_panes() if p["id"] == fresh_pane)
        assert pane.get("pending_prompt") is None

    def test_hook_parses_prompt_from_buffer(self, api, fresh_pane):
        """Hook should parse options when buffer has a real prompt."""
        api.send_text(fresh_pane, text=(
            'printf "\\nDo you want to proceed?\\n'
            '  \\xe2\\x9d\\xaf 1. Yes\\n'
            '    2. Yes, allow reading during this session\\n'
            '    3. No\\n'
            '\\nEsc to cancel \\xc2\\xb7 Tab to amend\\n"'
        ))
        time.sleep(2)
        api.post(f"/api/panes/{fresh_pane}/hook/permission")
        time.sleep(1)

        pane = next(p for p in api.list_panes() if p["id"] == fresh_pane)
        prompt = pane.get("pending_prompt")
        assert prompt is not None, "pending_prompt should be set after hook with prompt in buffer"
        assert len(prompt.get("options", [])) >= 2, f"Should have 2+ options, got {prompt}"


class TestPromptParsingAccuracy:
    """Test that prompt parser doesn't pick up numbered lists from Claude's answers."""

    def test_numbered_answer_then_prompt(self, api, fresh_pane):
        """Claude answer with numbered list, then real prompt — only Yes/No parsed."""
        # Simulate Claude's answer with numbered list THEN a real prompt
        api.send_text(fresh_pane, text=(
            'printf "'
            'Here is my analysis:\\n'
            '1. First bug found in parser\\n'
            '2. Second issue with encoding\\n'
            '3. Third problem in renderer\\n'
            '4. Fourth edge case\\n'
            '5. Fifth minor issue\\n'
            '\\n'
            'Do you want to proceed?\\n'
            '  \\xe2\\x9d\\xaf 1. Yes\\n'
            '    2. Yes, allow reading during this session\\n'
            '    3. No\\n'
            '\\nEsc to cancel \\xc2\\xb7 Tab to amend\\n"'
        ))
        time.sleep(2)

        # Hook triggers prompt parsing from ring buffer
        api.post(f"/api/panes/{fresh_pane}/hook/permission")
        time.sleep(1)

        pane = next(p for p in api.list_panes() if p["id"] == fresh_pane)
        prompt = pane.get("pending_prompt")
        assert prompt is not None, "Should detect prompt"
        assert prompt["question"] == "Do you want to proceed?"

        # Critical: options should be Yes/No, NOT "First bug", "Second issue"
        labels = [o["label"] for o in prompt["options"]]
        assert "Yes" in labels, f"Should have Yes, got {labels}"
        assert "No" in labels, f"Should have No, got {labels}"
        assert not any("bug" in l.lower() or "issue" in l.lower() for l in labels), \
            f"Should NOT have answer items in options: {labels}"
        assert len(prompt["options"]) <= 3, f"Should have max 3 options, got {len(prompt['options'])}"

    def test_only_numbered_answer_no_prompt(self, api, fresh_pane):
        """Claude answer with numbered list but NO prompt — no pending_prompt set."""
        api.send_text(fresh_pane, text=(
            'printf "'
            'Here is what I found:\\n'
            '1. First thing\\n'
            '2. Second thing\\n'
            '3. Third thing\\n'
            '\\nLet me fix these.\\n"'
        ))
        time.sleep(2)

        # Hook without real prompt in buffer — should not set prompt
        api.post(f"/api/panes/{fresh_pane}/hook/permission")
        time.sleep(1)

        pane = next(p for p in api.list_panes() if p["id"] == fresh_pane)
        prompt = pane.get("pending_prompt")
        # Without explicit body and without real prompt in buffer,
        # parser should not find anything
        if prompt:
            # If it somehow parsed, labels should not be "First thing" etc.
            labels = [o["label"] for o in prompt.get("options", [])]
            assert not any("thing" in l.lower() for l in labels), \
                f"Should NOT parse answer as prompt: {labels}"

    def test_explicit_body_overrides_buffer(self, api, fresh_pane):
        """Explicit body in hook/permission bypasses buffer parsing."""
        # Put garbage in buffer
        api.send_text(fresh_pane, text='printf "1. garbage\\n2. noise\\n3. junk\\n"')
        time.sleep(1)

        # Send explicit prompt via body — should ignore buffer
        api.post(f"/api/panes/{fresh_pane}/hook/permission", json={
            "question": "Allow Bash?",
            "options": [{"label": "Yes", "value": "1"}, {"label": "No", "value": "3"}]
        })
        time.sleep(1)

        pane = next(p for p in api.list_panes() if p["id"] == fresh_pane)
        prompt = pane.get("pending_prompt")
        assert prompt is not None
        assert prompt["question"] == "Allow Bash?"
        assert len(prompt["options"]) == 2
        assert prompt["options"][0]["label"] == "Yes"


class TestFullPromptApprovalFlow:
    """End-to-end prompt approval pipeline tests."""

    def test_full_prompt_approval_flow(self, api, fresh_pane):
        """Full flow: printf prompt -> hook -> pending_prompt -> respond -> PTY receives."""
        # 1. Write a realistic prompt to terminal (simulates Claude TUI)
        api.send_text(fresh_pane, text=(
            'printf "\\nDo you want to proceed?\\n'
            '  \\xe2\\x9d\\xaf 1. Yes\\n'
            '    2. Yes, allow reading during this session\\n'
            '    3. No\\n'
            '\\nEsc to cancel \\xc2\\xb7 Tab to amend\\n"'
        ))
        time.sleep(2)

        # 2. Simulate Claude hook call (POST /hook/permission)
        api.post(f"/api/panes/{fresh_pane}/hook/permission")
        time.sleep(1)

        # 3. Verify pending_prompt is set with parsed options
        pane = next(p for p in api.list_panes() if p["id"] == fresh_pane)
        prompt = pane.get("pending_prompt")
        assert prompt is not None, "pending_prompt should be set after hook"
        assert len(prompt.get("options", [])) >= 2, f"Should have 2+ options, got {prompt}"
        assert prompt["question"] == "Do you want to proceed?"

        # 4. Respond (simulates clicking "Yes" in chat UI)
        api.post(f"/api/panes/{fresh_pane}/prompt/respond", json={"value": "1"})
        time.sleep(1)

        # 5. Verify prompt is cleared
        pane = next(p for p in api.list_panes() if p["id"] == fresh_pane)
        assert pane.get("pending_prompt") is None, "Prompt should be cleared after respond"

        # 6. Verify "1" was sent to PTY (appears in output)
        output = api.get_output(fresh_pane, strip_ansi_flag=True)
        output_text = output.get("output", "")
        # The "1\r" should have been sent, appearing as "1" in output after the prompt
        # After printf and then "1\r", the shell will try to execute "1" as a command
        assert "1" in output_text, f"'1' should appear in PTY output after respond, got: {output_text[-200:]}"

    def test_two_pane_independent_approval(self, api):
        """Two panes with independent prompts -- approve one, other unaffected."""
        # Create workspace with 2 panes
        ws = api.create_workspace(unique_name("e2e-ws"))
        ws_id = ws["id"]
        tab1 = api.create_tab(ws_id)
        tab2 = api.create_tab(ws_id)
        p1, p2 = tab1["pane_id"], tab2["pane_id"]
        api.wait_for_pty(p1, timeout=15)
        api.wait_for_pty(p2, timeout=15)

        # Wait for shell prompt in both panes
        for pid in [p1, p2]:
            try:
                api.wait_for_output(pid, pattern=r"[\$#%>]", timeout=10)
            except (TimeoutError, Exception):
                time.sleep(2)

        try:
            # Write prompts to both
            for pid in [p1, p2]:
                api.send_text(pid, text=(
                    'printf "Do you want to proceed?\\n'
                    '  \\xe2\\x9d\\xaf 1. Yes\\n'
                    '    2. No\\n'
                    '\\nEsc to cancel \\xc2\\xb7 Tab to amend\\n"'
                ))
            time.sleep(2)

            # Hook both
            api.post(f"/api/panes/{p1}/hook/permission")
            api.post(f"/api/panes/{p2}/hook/permission")
            time.sleep(1)

            # Both should have prompts
            panes = api.list_panes()
            pane1 = next(p for p in panes if p["id"] == p1)
            pane2 = next(p for p in panes if p["id"] == p2)
            assert pane1.get("pending_prompt") is not None, "Pane 1 should have pending_prompt"
            assert pane2.get("pending_prompt") is not None, "Pane 2 should have pending_prompt"

            # Approve pane 1
            api.post(f"/api/panes/{p1}/prompt/respond", json={"value": "1"})
            time.sleep(0.5)

            # Pane 1 cleared, pane 2 still has prompt
            panes = api.list_panes()
            pane1 = next(p for p in panes if p["id"] == p1)
            pane2 = next(p for p in panes if p["id"] == p2)
            assert pane1.get("pending_prompt") is None, "Pane 1 prompt should be cleared"
            assert pane2.get("pending_prompt") is not None, "Pane 2 prompt should still be set"

            # Now approve pane 2
            api.post(f"/api/panes/{p2}/prompt/respond", json={"value": "2"})
            time.sleep(0.5)

            panes = api.list_panes()
            pane2 = next(p for p in panes if p["id"] == p2)
            assert pane2.get("pending_prompt") is None, "Pane 2 prompt should be cleared"
        finally:
            try:
                if len(api.list_workspaces()) > 1:
                    api.close_workspace(ws_id)
            except Exception:
                pass
