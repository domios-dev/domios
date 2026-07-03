/** Native-only IPC commands — honest web degradation via webCapability(). */
export const WEB_UNSUPPORTED: Record<string, string> = {
  get_pane_history_raw:
    "Native xterm scrollback replay on remount; web uses read_pty tail instead",
  materialize_dropped_files:
    "Native Tauri webview file-drop paths must be materialized on the host",
  read_clipboard_files:
    "OS clipboard file paths; web uses navigator.clipboard.read()",
  read_clipboard_text:
    "OS clipboard text reader; web uses navigator.clipboard.readText()",
};

/** Handled in invokeRaw without HTTP_MAP / CANNED membership. */
export const WEB_SPECIAL: Record<string, string> = {
  open_url: "Browser tab opens via window.open in invokeRaw",
};
