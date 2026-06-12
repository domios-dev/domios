"""Environment variable tests — verify PTY env injection."""

import re
import sys

from helpers import CodeTermAPI, echo_env


def env_output(api: CodeTermAPI, pane_id: str, var_name: str, expected: str) -> str:
    result = api.exec_cmd(pane_id, echo_env(var_name))
    output = result["output"]
    if expected not in output:
        output = api.wait_for_output(pane_id, pattern=re.escape(expected), timeout=5)
    return output


def test_codeterm_env(api: CodeTermAPI, fresh_pane: str):
    """echo $CODETERM should return '1'."""
    output = env_output(api, fresh_pane, "CODETERM", "1")
    assert "1" in output


def test_codeterm_pane_id_env(api: CodeTermAPI, fresh_pane: str):
    """echo $CODETERM_PANE_ID should return an 8-char hex matching the pane id."""
    output = env_output(api, fresh_pane, "CODETERM_PANE_ID", fresh_pane)
    # The pane_id is 8-char hex
    assert fresh_pane in output


def test_term_program_env(api: CodeTermAPI, fresh_pane: str):
    """echo $TERM_PROGRAM should return 'CodeTerm'."""
    output = env_output(api, fresh_pane, "TERM_PROGRAM", "CodeTerm")
    assert "CodeTerm" in output


def test_codeterm_socket_path_env(api: CodeTermAPI, fresh_pane: str):
    """$CODETERM_SOCKET_PATH should be set (non-empty)."""
    output = env_output(api, fresh_pane, "CODETERM_SOCKET_PATH", "codeterm").strip()
    # Output contains the echo command + the value. Look for a socket path pattern.
    if sys.platform != "win32":
        assert "codeterm" in output and ".sock" in output
    else:
        assert "codeterm" in output
