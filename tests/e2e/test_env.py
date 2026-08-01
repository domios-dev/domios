"""Environment variable tests — verify PTY env injection."""

import re
import sys

from helpers import CodeTermAPI, echo_env


def env_output(api: CodeTermAPI, tab_id: str, var_name: str, expected: str) -> str:
    result = api.exec_cmd(tab_id, echo_env(var_name))
    output = result["output"]
    if expected not in output:
        output = api.wait_for_output(tab_id, pattern=re.escape(expected), timeout=5)
    return output


def test_codeterm_env(api: CodeTermAPI, shared_eager_tab: str):
    """echo $CODETERM should return '1'."""
    output = env_output(api, shared_eager_tab, "CODETERM", "1")
    assert "1" in output


def test_codeterm_tab_id_env(api: CodeTermAPI, shared_eager_tab: str):
    """echo $CODETERM_TAB_ID should return an 8-char hex matching the tab id."""
    output = env_output(api, shared_eager_tab, "CODETERM_TAB_ID", shared_eager_tab)
    # The tab_id is 8-char hex
    assert shared_eager_tab in output


def test_term_program_env(api: CodeTermAPI, shared_eager_tab: str):
    """echo $TERM_PROGRAM should return 'CodeTerm'."""
    output = env_output(api, shared_eager_tab, "TERM_PROGRAM", "CodeTerm")
    assert "CodeTerm" in output


def test_codeterm_socket_path_env(api: CodeTermAPI, shared_eager_tab: str):
    """$CODETERM_SOCKET_PATH should be set (non-empty)."""
    output = env_output(api, shared_eager_tab, "CODETERM_SOCKET_PATH", "codeterm").strip()
    # Output contains the echo command + the value. Look for a socket path pattern.
    if sys.platform != "win32":
        assert "codeterm" in output and ".sock" in output
    else:
        assert "codeterm" in output
