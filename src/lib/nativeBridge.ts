import type { TransportClient } from "./transportClient";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { TrayIcon } from "@tauri-apps/api/tray";
import { open, save, type OpenDialogOptions, type SaveDialogOptions } from "@tauri-apps/plugin-dialog";

export type NativeUnsubscribe = () => void;
export type NotificationPermission = "granted" | "denied" | "prompt" | "unknown";
export type LocalRecordingCommand =
  | "local_recording_status" | "local_recording_set_enabled"
  | "local_recording_select_devices" | "local_recording_input_devices"
  | "local_recording_capture_stopped" | "local_recording_cancel"
  | "local_recording_manual_capture" | "local_recording_save_clip"
  | "local_recording_discard_clip" | "local_recording_commit";
export type FileDropEvent =
  | { type: "enter" | "drop"; paths: string[]; position: { x: number; y: number } }
  | { type: "over"; position: { x: number; y: number } }
  | { type: "leave" };

export interface NativeBridge {
  localRecording?: Record<LocalRecordingCommand, (args: Record<string, unknown>) => Promise<unknown>>;
  window: {
    minimize(): Promise<void>;
    toggleMaximize(): Promise<void>;
    close(): Promise<void>;
    show(): Promise<void>;
    focus(): Promise<void>;
    startDragging(): Promise<void>;
  } | undefined;
  tray: { setVisible(id: string, visible: boolean): Promise<void> } | undefined;
  dialogs: {
    open(options?: OpenDialogOptions): Promise<string | string[] | null>;
    save(options?: SaveDialogOptions): Promise<string | null>;
  } | undefined;
  notificationPermission: {
    state(): Promise<NotificationPermission>;
    request(enabling?: boolean): Promise<NotificationPermission>;
  } | undefined;
  updater: {
    check(force?: boolean): Promise<unknown>;
    status(): Promise<unknown>;
    install(): Promise<{ status: string }>;
  } | undefined;
  deepLinks: { subscribe(handler: (urls: string[]) => void): Promise<NativeUnsubscribe> } | undefined;
  clipboardFilePaths(): Promise<string[]>;
  materializeDroppedFiles(paths: string[]): Promise<string[]>;
  screenCapture: { capture(): Promise<MediaStream> } | undefined;
  fileDrop: { subscribe(handler: (event: FileDropEvent) => void): Promise<NativeUnsubscribe> } | undefined;
  resizeEvents: {
    onStart(handler: () => void): NativeUnsubscribe;
    onCommit(handler: () => void): NativeUnsubscribe;
  } | undefined;
  readClipboardText(): Promise<string>;
  readClipboard(): Promise<ClipboardItems>;
  openUrl(url: string): Promise<void>;
  openFolder(path: string): Promise<void>;
  revealFile(path: string): Promise<void>;
  autostart: { get(): Promise<boolean>; set(enabled: boolean): Promise<void> } | undefined;
  windowsBuildNumber(): Promise<number>;
  restartApp(): Promise<void>;
}

export class NativeCapabilityUnavailable extends Error {
  constructor(capability: string) {
    super(`Native capability unavailable: ${capability}`);
    this.name = "NativeCapabilityUnavailable";
  }
}

function permission(value: unknown): NotificationPermission {
  return value === "granted" || value === "denied" || value === "prompt" ? value : "unknown";
}

let daemonTransport: TransportClient | undefined;
export function bindNativeBridgeTransport(client: TransportClient): void {
  daemonTransport = client;
}

function subscribeNativeResize(event: string, handler: () => void): NativeUnsubscribe {
  let cancelled = false;
  let unsubscribe: NativeUnsubscribe | undefined;
  const registration = listen(event, () => { if (!cancelled) handler(); });
  void registration.then((fn) => { if (cancelled) fn(); else unsubscribe = fn; }).catch((error: unknown) => {
    if (!cancelled) console.warn("Native resize subscription failed", error);
  });
  return () => { cancelled = true; unsubscribe?.(); unsubscribe = undefined; };
}

export function createNativeBridge(native: boolean = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window): NativeBridge {
  const unavailable = (name: string): never => { throw new NativeCapabilityUnavailable(name); };
  const clipboard = () => {
    if (!globalThis.navigator?.clipboard) return unavailable("browser clipboard");
    return globalThis.navigator.clipboard;
  };
  return {
    localRecording: native ? {
      local_recording_status: (args) => invoke("local_recording_status", args),
      local_recording_set_enabled: (args) => invoke("local_recording_set_enabled", args),
      local_recording_select_devices: (args) => invoke("local_recording_select_devices", args),
      local_recording_input_devices: (args) => invoke("local_recording_input_devices", args),
      local_recording_capture_stopped: (args) => invoke("local_recording_capture_stopped", args),
      local_recording_cancel: (args) => invoke("local_recording_cancel", args),
      local_recording_manual_capture: (args) => invoke("local_recording_manual_capture", args),
      local_recording_save_clip: (args) => invoke("local_recording_save_clip", args),
      local_recording_discard_clip: (args) => invoke("local_recording_discard_clip", args),
      local_recording_commit: (args) => invoke("local_recording_commit", args),
    } : undefined,
    window: native ? {
      minimize: () => getCurrentWindow().minimize(),
      toggleMaximize: () => getCurrentWindow().toggleMaximize(),
      close: () => getCurrentWindow().close(),
      show: () => getCurrentWindow().show(),
      focus: () => getCurrentWindow().setFocus(),
      startDragging: () => getCurrentWindow().startDragging(),
    } : undefined,
    tray: native ? {
      async setVisible(id, visible) {
        const tray = await TrayIcon.getById(id);
        if (!tray) return unavailable("tray");
        await tray.setVisible(visible);
      },
    } : undefined,
    dialogs: native ? { open, save } : undefined,
    notificationPermission: native ? {
      state: async () => permission(await invoke("notify_permission_state")),
      request: async (enabling = true) => permission(await invoke("establish_notify_permission", { enabling })),
    } : undefined,
    updater: native ? {
      check: (force = false) => invoke("check_for_update", { force }),
      status: () => invoke("get_update_status"),
      install: () => invoke("install_app_update"),
    } : undefined,
    deepLinks: undefined,
    async clipboardFilePaths() { if (!native) return unavailable("clipboard file paths"); return invoke("read_clipboard_files"); },
    async materializeDroppedFiles(paths) { if (!native) return unavailable("native file drop"); return invoke("materialize_dropped_files", { paths }); },
    screenCapture: typeof globalThis.navigator?.mediaDevices?.getDisplayMedia === "function" ? {
      capture: () => navigator.mediaDevices.getDisplayMedia({ video: true }),
    } : undefined,
    fileDrop: native ? {
      subscribe: (handler) => getCurrentWebview().onDragDropEvent((event) => handler(event.payload)),
    } : undefined,
    resizeEvents: native ? {
      onStart: (handler) => subscribeNativeResize("terminal-window-resize-start", handler),
      onCommit: (handler) => subscribeNativeResize("terminal-window-resize-commit", handler),
    } : undefined,
    readClipboardText: () => native ? invoke("read_clipboard_text") : clipboard().readText(),
    readClipboard: () => clipboard().read(),
    async openUrl(url) {
      if (native) return invoke("open_url", { url });
      globalThis.window.open(url, "_blank", "noopener,noreferrer");
    },
    async openFolder(path) { if (!native) return unavailable("open folder"); return invoke("open_folder", { path }); },
    async revealFile(path) { if (!native) return unavailable("reveal file"); return invoke("reveal_file", { path }); },
    autostart: native ? {
      get: () => invoke("get_autostart_enabled"),
      set: (enabled) => invoke("set_autostart_enabled", { enabled }),
    } : undefined,
    async windowsBuildNumber() { if (!native) return unavailable("Windows build number"); return invoke("get_windows_build_number"); },
    async restartApp() {
      if (!daemonTransport) return unavailable("daemon restart requires HTTP transport");
      await daemonTransport.localJson("POST", "/api/app/restart");
    },
  };
}

export const nativeBridge: NativeBridge = createNativeBridge();

export async function nativeFolderDialog(options: { directory: true; multiple: false; title: string }): Promise<string | null> {
  if (!nativeBridge.dialogs) throw new NativeCapabilityUnavailable("folder picker");
  const folder = await nativeBridge.dialogs.open(options);
  return typeof folder === "string" ? folder : null;
}
