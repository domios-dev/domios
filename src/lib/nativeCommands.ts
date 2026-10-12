import { nativeBridge, NativeCapabilityUnavailable, type LocalRecordingCommand } from "./nativeBridge";
import type { CommandAdapter, CommandArgs } from "./transportCommands";
const requireCapability = <T>(value: T | undefined, name: string): T => {
  if (!value) throw new NativeCapabilityUnavailable(name);
  return value;
};
export const NATIVE_COMMANDS: Record<string, CommandAdapter> = {
  local_recording_status: (a) => localRecording("local_recording_status", a),
  local_recording_set_enabled: (a) => localRecording("local_recording_set_enabled", a),
  local_recording_select_devices: (a) => localRecording("local_recording_select_devices", a),
  local_recording_input_devices: (a) => localRecording("local_recording_input_devices", a),
  local_recording_capture_stopped: (a) => localRecording("local_recording_capture_stopped", a),
  local_recording_cancel: (a) => localRecording("local_recording_cancel", a),
  local_recording_manual_capture: (a) => localRecording("local_recording_manual_capture", a),
  local_recording_save_clip: (a) => localRecording("local_recording_save_clip", a),
  local_recording_discard_clip: (a) => localRecording("local_recording_discard_clip", a),
  local_recording_commit: (a) => localRecording("local_recording_commit", a),
  open_url: (a) => nativeBridge.openUrl(String(a.url ?? "")),
  open_folder: (a) => nativeBridge.openFolder(String(a.path ?? "")),
  reveal_file: (a) => nativeBridge.revealFile(String(a.path ?? "")),
  read_clipboard_text: () => nativeBridge.readClipboardText(),
  read_clipboard_files: () => nativeBridge.clipboardFilePaths(),
  materialize_dropped_files: (a) => nativeBridge.materializeDroppedFiles(a.paths as string[]),
  notify_permission_state: () => requireCapability(nativeBridge.notificationPermission, "notification permission").state(),
  establish_notify_permission: (a) => requireCapability(nativeBridge.notificationPermission, "notification permission").request(a.enabling !== false),
  get_autostart_enabled: () => requireCapability(nativeBridge.autostart, "autostart").get(),
  set_autostart_enabled: (a) => requireCapability(nativeBridge.autostart, "autostart").set(a.enabled === true),
  get_windows_build_number: () => nativeBridge.windowsBuildNumber(),
  check_for_update: (a) => requireCapability(nativeBridge.updater, "app updater").check(a.force === true),
  get_update_status: () => requireCapability(nativeBridge.updater, "app updater").status(),
  install_app_update: () => requireCapability(nativeBridge.updater, "app updater").install(),
};
function localRecording(command: LocalRecordingCommand, args: CommandArgs): Promise<unknown> {
  return requireCapability(nativeBridge.localRecording, "local recording")[command](args);
}
export function nativeCommand(command: string, args: CommandArgs): Promise<unknown> | undefined {
  if (!Object.prototype.hasOwnProperty.call(NATIVE_COMMANDS, command)) return undefined;
  return Promise.resolve().then(() => NATIVE_COMMANDS[command](args));
}
