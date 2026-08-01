"""Eager tab PTY behavior for headless/API consumers."""

from helpers import CodeTermAPI, shell_cmd, unique_name


def test_eager_non_active_tab_has_pty_and_accepts_send_text(api: CodeTermAPI):
    ws = api.create_workspace(unique_name("e2e-ws-eager"))
    ws_id = ws["id"]
    try:
        first = api.create_tab(ws_id, eager=True)
        assert first["tab_id"]
        assert first.get("pty_id")

        second = api.create_tab(ws_id, eager=True)
        tab_id = second["tab_id"]
        assert tab_id
        assert second.get("pty_id")

        tab = next(p for p in api.list_tabs() if p["id"] == tab_id)
        assert tab.get("pty_id")
        assert not tab.get("focused", False)

        # Shell readiness gate, same as fresh_tab: typing before the shell
        # reads stdin loses the keystrokes (cold PowerShell takes seconds).
        api.wait_for_output(tab_id, pattern=r"[\$#%>]", timeout=15)

        suffix = unique_name("").lstrip("-")
        assembled_marker = f"EAGER_OK_{suffix}"
        api.send_text(
            tab_id,
            text=shell_cmd(
                f"printf 'EAGER_%s\\n' 'OK_{suffix}'",
                f'Write-Output ("EAGER_" + "OK_{suffix}")',
            ),
        )
        output = api.wait_for_output(tab_id, pattern=assembled_marker, timeout=15)
        assert assembled_marker in output
    finally:
        api.close_workspace(ws_id)
