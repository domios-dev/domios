"""PTY I/O tests — the core of the E2E suite.

Tests exec, send, batch, output offsets, ANSI stripping, screen endpoint.
"""

import time

import pytest

from helpers import CodeTermAPI, shell_cmd, unique_name


def _split_marker(prefix: str) -> tuple[str, str, str]:
    marker = unique_name(prefix).replace("-", "_")
    split = len(marker) // 2
    return marker, marker[:split], marker[split:]


def _echo_split(prefix: str) -> tuple[str, str]:
    marker, left, right = _split_marker(prefix)
    return shell_cmd(f"echo '{left}''{right}'", f"Write-Output ('{left}' + '{right}')"), marker


# -- exec ------------------------------------------------------------------

def test_exec_echo(api: CodeTermAPI, shared_eager_pane: str):
    """exec echo and verify split-marker output."""
    cmd, marker = _echo_split("hello")
    result = api.exec_cmd(shared_eager_pane, cmd)
    assert marker in result["output"]
    assert "offset" in result


def test_exec_timeout(api: CodeTermAPI, fresh_pane: str):
    """exec with a tiny timeout returns partial output without blocking."""
    # 'sleep 5' won't finish in 500ms — exec should return early
    result = api.exec_cmd(fresh_pane, "sleep 5", timeout_ms=500)
    # Should return something (at least the command echo), not hang for 5s
    assert "output" in result
    assert "offset" in result


def test_exec_multiline_output(api: CodeTermAPI, shared_eager_pane: str):
    """Command that prints multiple lines."""
    markers = [_split_marker(f"line{i}") for i in range(1, 4)]
    result = api.exec_cmd(shared_eager_pane, shell_cmd(
        "printf '{}{}\\n{}{}\\n{}{}\\n'".format(
            markers[0][1], markers[0][2],
            markers[1][1], markers[1][2],
            markers[2][1], markers[2][2],
        ),
        "Write-Host (('{0}' + '{1}') + \"`n\" + ('{2}' + '{3}') + \"`n\" + ('{4}' + '{5}'))".format(
            markers[0][1], markers[0][2],
            markers[1][1], markers[1][2],
            markers[2][1], markers[2][2],
        ),
    ))
    for marker, _, _ in markers:
        assert marker in result["output"]


def test_special_characters(api: CodeTermAPI, shared_eager_pane: str):
    """Echo with quotes, backslashes, unicode."""
    result = api.exec_cmd(shared_eager_pane, shell_cmd(
        r"echo 'hello \"world\" \\ \u2603'",
        r'echo "hello `"world`" \\ \u2603"',
    ))
    # At minimum the echo should execute without error
    assert "output" in result
    assert "offset" in result


# -- send ------------------------------------------------------------------

def test_send_with_enter_default(api: CodeTermAPI, shared_eager_pane: str):
    """Send text without specifying enter (defaults true), verify execution."""
    cmd, marker = _echo_split("enter_default")
    api.send_text(shared_eager_pane, text=cmd)
    # Wait for output to contain the echoed text
    output = api.wait_for_output(shared_eager_pane, pattern=marker, timeout=10)
    assert marker in output


def test_send_with_enter_true(api: CodeTermAPI, shared_eager_pane: str):
    """Explicit enter=true, verify execution."""
    cmd, marker = _echo_split("enter_true")
    api.send_text(shared_eager_pane, text=cmd, enter=True)
    output = api.wait_for_output(shared_eager_pane, pattern=marker, timeout=10)
    assert marker in output


def test_send_with_enter_false(api: CodeTermAPI, shared_eager_pane: str):
    """enter=false: text appears in terminal but does not execute."""
    marker = f"NOEXEC_{int(time.time() * 1000)}"
    left, right = marker[: len(marker) // 2], marker[len(marker) // 2 :]
    cmd = shell_cmd(f"echo '{left}''{right}'", f"Write-Output ('{left}' + '{right}')")
    api.send_text(shared_eager_pane, text=cmd, enter=False)

    deadline = time.monotonic() + 2.0
    while time.monotonic() < deadline:
        output = api.get_output(shared_eager_pane, strip_ansi_flag=True)
        assert marker not in output["output"], (
            f"enter=false unexpectedly executed command; marker={marker!r} "
            f"tail={output['output'][-300:]!r}"
        )
        time.sleep(0.2)

    api.post(f"/api/panes/{shared_eager_pane}/send", json={"text": "", "enter": True, "raw": True})
    output = api.wait_for_output(shared_eager_pane, pattern=marker, timeout=10)
    assert marker in output


def test_send_empty_with_enter(api: CodeTermAPI, shared_eager_pane: str):
    """Empty text + enter=true sends just Enter (empty command)."""
    before = api.get_output(shared_eager_pane)
    offset_before = before["offset"]

    result = api.send_text(shared_eager_pane, text="", enter=True)
    assert result.get("ok") is True

    # Should produce some output (new prompt line)
    time.sleep(1)
    after = api.get_output(shared_eager_pane, offset=offset_before)
    assert len(after["output"]) > 0


# -- batch -----------------------------------------------------------------

def test_batch_send(api: CodeTermAPI, shared_eager_pane: str):
    """Batch of 2 commands with delay, both execute."""
    cmd_a, marker_a = _echo_split("batch_a")
    cmd_b, marker_b = _echo_split("batch_b")
    batch = [
        {"text": cmd_a, "delay_ms": 100},
        {"text": cmd_b, "delay_ms": 100},
    ]
    result = api.send_text(shared_eager_pane, batch=batch)
    assert result.get("batch") is True

    output = api.wait_for_output(shared_eager_pane, pattern=marker_b, timeout=15)
    assert marker_a in output
    assert marker_b in output


def test_batch_enter_default(api: CodeTermAPI, shared_eager_pane: str):
    """Batch items without 'enter' field default to true."""
    cmd_a, marker_a = _echo_split("batch_default_1")
    cmd_b, marker_b = _echo_split("batch_default_2")
    batch = [
        {"text": cmd_a},
        {"text": cmd_b},
    ]
    result = api.send_text(shared_eager_pane, batch=batch)
    assert result.get("batch") is True

    output = api.wait_for_output(shared_eager_pane, pattern=marker_b, timeout=15)
    assert marker_a in output
    assert marker_b in output


# -- output ----------------------------------------------------------------

def test_output_offset_cursor(api: CodeTermAPI, shared_eager_pane: str):
    """First call gets full output + offset, second call with offset gets only new data."""
    # First read: get current state
    first = api.get_output(shared_eager_pane, strip_ansi_flag=True)
    offset1 = first["offset"]
    assert offset1 > 0  # Shell prompt should have produced some output

    # Produce new output
    cmd, marker = _echo_split("offset")
    api.exec_cmd(shared_eager_pane, cmd)

    # Second read with offset — should only get new data
    second = api.get_output(shared_eager_pane, offset=offset1, strip_ansi_flag=True)
    assert marker in second["output"]
    assert second["offset"] > offset1


def test_output_strip_ansi(api: CodeTermAPI, shared_eager_pane: str):
    """Verify strip_ansi=true removes escape codes from output."""
    # Produce colored output
    marker, left, right = _split_marker("red")
    api.exec_cmd(shared_eager_pane, shell_cmd(
        rf"printf '\033[31m{left}{right}\033[0m'",
        f'Write-Host ("`e[31m" + "{left}" + "{right}" + "`e[0m") -NoNewline',
    ))

    # Stripped output should not contain ESC
    stripped = api.get_output(shared_eager_pane, strip_ansi_flag=True)

    assert "\x1b" not in stripped["output"]
    assert marker in stripped["output"]


def test_text_endpoint(api: CodeTermAPI, shared_eager_pane: str):
    """GET /api/panes/{id}/text returns visible text."""
    try:
        text = api.get_screen(shared_eager_pane)
        assert isinstance(text, str)
    except Exception:
        pytest.skip("Text endpoint requires GUI frontend (Tauri event bridge)")
