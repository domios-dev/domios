/** Native-only IPC commands — honest web degradation via webCapability(). */
export const WEB_UNSUPPORTED: Record<string, string> = {
  get_recovery_state:
    "Native crash recovery state is sourced from the desktop supervisor",
  get_tab_history_raw:
    "Native xterm scrollback replay on remount; web uses read_pty tail instead",
  materialize_dropped_files:
    "Native Tauri webview file-drop paths must be materialized on the host",
  read_clipboard_files:
    "OS clipboard file paths; web uses navigator.clipboard.read()",
  read_clipboard_text:
    "OS clipboard text reader; web uses navigator.clipboard.readText()",
  read_html_preview_file:
    "Local file-backed HTML previews are available only in the desktop app",
  read_local_preview_file:
    "Local page, image, and document previews are available only in the desktop app",
  frontend_ready:
    "Desktop startup readiness acknowledgement has no browser equivalent",
  frontend_readiness_generation:
    "Desktop startup readiness generation has no browser equivalent",
  get_rest_api_status:
    "REST listener recovery is owned by the desktop supervisor",
  restart_rest_api:
    "An HTTP client cannot restart the listener carrying its own request",
  check_provider_integration:
    "Provider integration health reads native host configuration",
  install_provider_integration:
    "Provider integration repair changes native host configuration",
  uninstall_provider_integration:
    "Provider integration removal changes native host configuration",
};

/** Handled in invokeRaw without HTTP_MAP / CANNED membership. */
export const WEB_SPECIAL: Record<string, string> = {
  open_url: "Browser tab opens via window.open in invokeRaw",
};
