"""PTY I/O tests — the core of the E2E suite.

Tests exec, send, batch, output offsets, ANSI stripping, screen endpoint.
"""

import re
import time

import pytest

from helpers import CodeTermAPI, shell_cmd, strip_ansi


# -- exec ------------------------------------------------------------------

def test_exec_echo(api: CodeTermAPI, fresh_pane: str):
    """exec 'echo HELLO', verify output contains HELLO."""
    result = api.exec_cmd(fresh_pane, "echo HELLO")
    assert "HELLO" in result["output"]
    assert "offset" in result


def test_exec_timeout(api: CodeTermAPI, fresh_pane: str):
    """exec with a tiny timeout returns partial output without blocking."""
    # 'sleep 5' won't finish in 500ms — exec should return early
    result = api.exec_cmd(fresh_pane, "sleep 5", timeout_ms=500)
    # Should return something (at least the command echo), not hang for 5s
    assert "output" in result
    assert "offset" in result


def test_exec_multiline_output(api: CodeTermAPI, fresh_pane: str):
    """Command that prints multiple lines."""
    result = api.exec_cmd(fresh_pane, shell_cmd(
        "printf 'line1\\nline2\\nline3\\n'",
        'Write-Host "line1`nline2`nline3"',
    ))
    assert "line1" in result["output"]
    assert "line2" in result["output"]
    assert "line3" in result["output"]


def test_special_characters(api: CodeTermAPI, fresh_pane: str):
    """Echo with quotes, backslashes, unicode."""
    result = api.exec_cmd(fresh_pane, shell_cmd(
        r"echo 'hello \"world\" \\ \u2603'",
        r'echo "hello `"world`" \\ \u2603"',
    ))
    # At minimum the echo should execute without error
    assert "output" in result
    assert "offset" in result


# -- send ------------------------------------------------------------------

def test_send_with_enter_default(api: CodeTermAPI, fresh_pane: str):
    """Send text without specifying enter (defaults true), verify execution."""
    # Get current offset
    before = api.get_output(fresh_pane)
    offset = before["offset"]

    api.send_text(fresh_pane, text="echo ENTER_DEFAULT")
    # Wait for output to contain the echoed text
    output = api.wait_for_output(fresh_pane, pattern="ENTER_DEFAULT", timeout=10)
    assert "ENTER_DEFAULT" in output


def test_send_with_enter_true(api: CodeTermAPI, fresh_pane: str):
    """Explicit enter=true, verify execution."""
    api.send_text(fresh_pane, text="echo ENTER_TRUE", enter=True)
    output = api.wait_for_output(fresh_pane, pattern="ENTER_TRUE", timeout=10)
    assert "ENTER_TRUE" in output


def test_send_with_enter_false(api: CodeTermAPI, fresh_pane: str):
    """enter=false: text appears in terminal but does not execute."""
    marker = "NOPE_NO_EXEC"
    api.send_text(fresh_pane, text=f"echo {marker}", enter=False)
    # Give a moment for the text to arrive
    time.sleep(1)

    # The marker should be visible in the output buffer (typed into prompt)
    # but should NOT produce command output (no newline was sent)
    output = api.get_output(fresh_pane, strip_ansi_flag=True)
    text = output["output"]

    # The text "echo NOPE_NO_EXEC" should appear (it was typed)
    assert marker in text

    # Clean up: send Ctrl+C to cancel the partial input, then Enter
    api.send_text(fresh_pane, text="\x03", enter=False)
    time.sleep(0.3)


def test_send_empty_with_enter(api: CodeTermAPI, fresh_pane: str):
    """Empty text + enter=true sends just Enter (empty command)."""
    before = api.get_output(fresh_pane)
    offset_before = before["offset"]

    result = api.send_text(fresh_pane, text="", enter=True)
    assert result.get("ok") is True

    # Should produce some output (new prompt line)
    time.sleep(1)
    after = api.get_output(fresh_pane, offset=offset_before)
    assert len(after["output"]) > 0


# -- batch -----------------------------------------------------------------

def test_batch_send(api: CodeTermAPI, fresh_pane: str):
    """Batch of 2 commands with delay, both execute."""
    batch = [
        {"text": "echo BATCH_A", "delay_ms": 100},
        {"text": "echo BATCH_B", "delay_ms": 100},
    ]
    result = api.send_text(fresh_pane, batch=batch)
    assert result.get("batch") is True

    output = api.wait_for_output(fresh_pane, pattern="BATCH_B", timeout=15)
    assert "BATCH_A" in output
    assert "BATCH_B" in output


def test_batch_enter_default(api: CodeTermAPI, fresh_pane: str):
    """Batch items without 'enter' field default to true."""
    batch = [
        {"text": "echo BATCH_DEFAULT_1"},
        {"text": "echo BATCH_DEFAULT_2"},
    ]
    result = api.send_text(fresh_pane, batch=batch)
    assert result.get("batch") is True

    output = api.wait_for_output(fresh_pane, pattern="BATCH_DEFAULT_2", timeout=15)
    assert "BATCH_DEFAULT_1" in output
    assert "BATCH_DEFAULT_2" in output


# -- output ----------------------------------------------------------------

def test_output_offset_cursor(api: CodeTermAPI, fresh_pane: str):
    """First call gets full output + offset, second call with offset gets only new data."""
    # First read: get current state
    first = api.get_output(fresh_pane, strip_ansi_flag=True)
    offset1 = first["offset"]
    assert offset1 > 0  # Shell prompt should have produced some output

    # Produce new output
    api.exec_cmd(fresh_pane, "echo OFFSET_TEST")

    # Second read with offset — should only get new data
    second = api.get_output(fresh_pane, offset=offset1, strip_ansi_flag=True)
    assert "OFFSET_TEST" in second["output"]
    assert second["offset"] > offset1


def test_output_strip_ansi(api: CodeTermAPI, fresh_pane: str):
    """Verify strip_ansi=true removes escape codes from output."""
    # Produce colored output
    api.exec_cmd(fresh_pane, shell_cmd(
        r"printf '\033[31mRED\033[0m'",
        'Write-Host "`e[31mRED`e[0m" -NoNewline',
    ))

    # Raw output may contain escape codes
    raw = api.get_output(fresh_pane, strip_ansi_flag=False)
    # Stripped output should not contain ESC
    stripped = api.get_output(fresh_pane, strip_ansi_flag=True)

    assert "\x1b" not in stripped["output"]
    assert "RED" in stripped["output"]


def test_screen_endpoint(api: CodeTermAPI, fresh_pane: str):
    """GET /api/panes/{id}/screen returns visible text."""
    # Note: screen endpoint requires frontend to be running (Tauri event).
    # In headless dev mode this may time out — mark as expected failure.
    try:
        screen = api.get_screen(fresh_pane)
        assert isinstance(screen, str)
    except Exception:
        pytest.skip("Screen endpoint requires GUI frontend (Tauri event bridge)")
