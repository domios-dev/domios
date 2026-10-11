/** Native-only IPC commands — honest web degradation via webCapability(). */
export const WEB_UNSUPPORTED: Record<string, string> = {
  reveal_file: "Native OS file manager required",
  open_folder: "Native OS file manager required",
  materialize_dropped_files: "Native OS file-drop paths required",
  read_clipboard_files: "Browser clipboard supplies bytes rather than OS paths",
  notify_permission_state: "Native OS notification permission required",
  establish_notify_permission: "Native OS notification prompt required",
  get_autostart_enabled: "Native autostart required",
  set_autostart_enabled: "Native autostart required",
  get_windows_build_number: "Native Windows version required",
  check_for_update: "Native app updater required",
  get_update_status: "Native app updater required",
  install_app_update: "Native app installer required",
};
export const WEB_SPECIAL = { open_url: "Browser opens a URL through nativeBridge's browser capability" };
