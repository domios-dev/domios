"""Environment variable tests — verify PTY env injection."""

import sys

from helpers import CodeTermAPI, echo_env


def test_codeterm_env(api: CodeTermAPI, fresh_pane: str):
    """echo $CODETERM should return '1'."""
    result = api.exec_cmd(fresh_pane, echo_env("CODETERM"))
    assert "1" in result["output"]


def test_codeterm_pane_id_env(api: CodeTermAPI, fresh_pane: str):
    """echo $CODETERM_PANE_ID should return an 8-char hex matching the pane id."""
    result = api.exec_cmd(fresh_pane, echo_env("CODETERM_PANE_ID"))
    # The pane_id is 8-char hex
    assert fresh_pane in result["output"]


def test_term_program_env(api: CodeTermAPI, fresh_pane: str):
    """echo $TERM_PROGRAM should return 'CodeTerm'."""
    result = api.exec_cmd(fresh_pane, echo_env("TERM_PROGRAM"))
    assert "CodeTerm" in result["output"]


def test_codeterm_socket_path_env(api: CodeTermAPI, fresh_pane: str):
    """$CODETERM_SOCKET_PATH should be set (non-empty)."""
    result = api.exec_cmd(fresh_pane, echo_env("CODETERM_SOCKET_PATH"))
    output = result["output"].strip()
    # Output contains the echo command + the value. Look for a socket path pattern.
    if sys.platform != "win32":
        assert "codeterm" in output and ".sock" in output
    else:
        assert "codeterm" in output
