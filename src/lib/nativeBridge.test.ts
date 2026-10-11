import type { TransportClient } from "./transportClient";
import { beforeEach, describe, expect, it, vi } from "vitest";
const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(), minimize: vi.fn(), open: vi.fn(), drop: vi.fn(), tray: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke, isTauri: () => false }));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ minimize: mocks.minimize }) }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: () => ({ onDragDropEvent: mocks.drop }) }));
vi.mock("@tauri-apps/api/tray", () => ({ TrayIcon: { getById: mocks.tray } }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: mocks.open, save: vi.fn() }));
import { bindNativeBridgeTransport, createNativeBridge, NativeCapabilityUnavailable } from "./nativeBridge";

beforeEach(() => vi.clearAllMocks());
describe("nativeBridge", () => {
  it("makes OS capabilities explicitly unavailable in a browser", async () => {
    const bridge = createNativeBridge(false);
    expect([bridge.window, bridge.tray, bridge.dialogs, bridge.notificationPermission, bridge.updater, bridge.deepLinks, bridge.fileDrop, bridge.resizeEvents, bridge.autostart]).toEqual(Array(9).fill(undefined));
    await expect(bridge.openFolder("/host")).rejects.toBeInstanceOf(NativeCapabilityUnavailable);
    await expect(bridge.clipboardFilePaths()).rejects.toBeInstanceOf(NativeCapabilityUnavailable);
    expect(mocks.invoke).not.toHaveBeenCalled();
  });
  it("uses browser clipboard APIs without invoking the host", async () => {
    const readText = vi.fn().mockResolvedValue("browser text");
    const read = vi.fn().mockResolvedValue([]);
    vi.stubGlobal("navigator", { clipboard: { readText, read } });
    const bridge = createNativeBridge(false);
    expect(await bridge.readClipboardText()).toBe("browser text");
    expect(await bridge.readClipboard()).toEqual([]);
    expect(mocks.invoke).not.toHaveBeenCalled();
    vi.unstubAllGlobals();
  });
  it("uses the native capability even for a daemon-hosted shell", async () => {
    const bridge = createNativeBridge(true);
    await bridge.window?.minimize();
    await bridge.dialogs?.open({ directory: true });
    mocks.invoke.mockResolvedValue(["/file"]);
    expect(await bridge.clipboardFilePaths()).toEqual(["/file"]);
    expect(mocks.invoke).toHaveBeenCalledWith("read_clipboard_files");
    expect(mocks.minimize).toHaveBeenCalledOnce();
    expect(mocks.open).toHaveBeenCalledWith({ directory: true });
  });
  it("preserves permission refusal and does not prompt on disable", async () => {
    mocks.invoke.mockResolvedValue("denied");
    const bridge = createNativeBridge(true);
    expect(await bridge.notificationPermission?.request(false)).toBe("denied");
    expect(mocks.invoke).toHaveBeenCalledWith("establish_notify_permission", { enabling: false });
    mocks.invoke.mockResolvedValue("unexpected");
    expect(await bridge.notificationPermission?.state()).toBe("unknown");
  });
  it("cleans a native resize subscription cancelled before registration completes", async () => {
    let resolve!: (fn: () => void) => void;
    const registration = new Promise<() => void>((done) => { resolve = done; });
    mocks.listen.mockReturnValue(registration);
    const handler = vi.fn();
    const unsubscribe = createNativeBridge(true).resizeEvents!.onStart(handler);
    unsubscribe();
    const cleanup = vi.fn();
    resolve(cleanup);
    await registration;
    expect(cleanup).toHaveBeenCalledOnce();
    const [event, callback] = mocks.listen.mock.calls[0];
    expect(event).toBe("terminal-window-resize-start");
    callback();
    expect(handler).not.toHaveBeenCalled();
  });
  it("keeps the native updater response discriminant", async () => {
    mocks.invoke.mockResolvedValue({ status: "already_running" });
    expect(await createNativeBridge(true).updater?.install()).toEqual({ status: "already_running" });
    expect(mocks.invoke).toHaveBeenCalledWith("install_app_update");
  });
  it("routes application restart over the bound daemon transport", async () => {
    const localJson = vi.fn().mockResolvedValue({ ok: true });
    const client: TransportClient = {
      json: vi.fn(), localJson, upload: vi.fn(), subscribe: vi.fn(), currentToken: () => "token", origin: () => "http://127.0.0.1:7686",
    };
    bindNativeBridgeTransport(client);
    await createNativeBridge(true).restartApp();
    expect(localJson).toHaveBeenCalledWith("POST", "/api/app/restart");
    expect(mocks.invoke).not.toHaveBeenCalled();
  });
  it("offers display capture only when the browser capability exists", async () => {
    const capture = vi.fn().mockResolvedValue("stream");
    vi.stubGlobal("navigator", { mediaDevices: { getDisplayMedia: capture } });
    expect(await createNativeBridge(false).screenCapture?.capture()).toBe("stream");
    expect(capture).toHaveBeenCalledWith({ video: true });
    vi.unstubAllGlobals();
  });
});
