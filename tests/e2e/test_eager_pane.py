"""Eager pane PTY behavior for headless/API consumers."""

from helpers import CodeTermAPI, shell_cmd, unique_name


def test_eager_non_active_tab_has_pty_and_accepts_send_text(api: CodeTermAPI):
    ws = api.create_workspace(unique_name("e2e-ws-eager"))
    ws_id = ws["id"]
    try:
        first = api.create_tab(ws_id, eager=True)
        assert first["pane_id"]
        assert first.get("pty_id")

        second = api.create_tab(ws_id, eager=True)
        pane_id = second["pane_id"]
        assert pane_id
        assert second.get("pty_id")

        pane = next(p for p in api.list_panes() if p["id"] == pane_id)
        assert pane.get("pty_id")
        assert not pane.get("focused", False)

        # Shell readiness gate, same as fresh_pane: typing before the shell
        # reads stdin loses the keystrokes (cold PowerShell takes seconds).
        api.wait_for_output(pane_id, pattern=r"[\$#%>]", timeout=15)

        suffix = unique_name("").lstrip("-")
        assembled_marker = f"EAGER_OK_{suffix}"
        api.send_text(
            pane_id,
            text=shell_cmd(
                f"printf 'EAGER_%s\\n' 'OK_{suffix}'",
                f'Write-Output ("EAGER_" + "OK_{suffix}")',
            ),
        )
        output = api.wait_for_output(pane_id, pattern=assembled_marker, timeout=15)
        assert assembled_marker in output
    finally:
        api.close_workspace(ws_id)
