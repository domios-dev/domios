import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { AppConfig, Notification } from "../lib/types";
import { UpdateProgressLayer } from "./UpdateProgressLayer";
import { SecretsTab } from "./Settings/SecretsTab";
import { usePluginActivity } from "../lib/usePluginActivity";
import { NotificationToastContainer } from "./Notifications/NotificationToast";

const fakes = vi.hoisted(() => ({
  upsert: vi.fn(), add: vi.fn(), dismiss: vi.fn(), invoke: vi.fn(),
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
  update: { status: "up-to-date" as string, latestVersion: "2", error: null as string | null, toastOpen: true, blocking: false, dismissUpdateToast: vi.fn(), applyUpdate: vi.fn() },
}));
vi.mock("../store/multiplexer", () => ({
  useMultiplexer: () => ({ upsertToast: fakes.upsert, addNotification: fakes.add, dismissToast: fakes.dismiss }),
  useMuxState: () => ({ workspaces: [], active_workspace_id: null }),
}));
vi.mock("../lib/bridge", () => ({
  invoke: fakes.invoke,
  listen: vi.fn(async (name: string, handler: (event: { payload: unknown }) => void) => {
    fakes.listeners.set(name, handler);
    return () => fakes.listeners.delete(name);
  }),
}));
vi.mock("../lib/nativeBridge", () => ({ nativeBridge: { openFolder: (path: string) => fakes.invoke("open_folder", { path }) } }));
vi.mock("../hooks/useUpdate", () => ({ useUpdate: () => fakes.update }));

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  localStorage.clear();
  fakes.listeners.clear();
  fakes.invoke.mockResolvedValue({});
  fakes.update.status = "up-to-date";
  fakes.update.error = null;
  fakes.update.blocking = false;
  fakes.update.toastOpen = true;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});
const render = async (node: ReactNode) => { await act(async () => root.render(node)); };
async function showCapturedToast(source: "upsert" | "add" = "upsert") {
  const calls = fakes[source].mock.calls;
  const value = calls[calls.length - 1]?.[0] as Notification;
  expect(value).toBeDefined();
  await render(<NotificationToastContainer toasts={[{ ...value, id: value.id ?? "local", timestamp: 1, read: false }]} onDismiss={fakes.dismiss} />);
  expect(host.querySelectorAll('[data-testid="toast-host"]')).toHaveLength(1);
  expect(host.querySelectorAll('button[aria-label="Dismiss"]')).toHaveLength(1);
  return value;
}

test.each(["available", "up-to-date", "checking", "downloading"])("%s update state creates no result toast", async (status) => {
  fakes.update.status = status;
  await render(<UpdateProgressLayer />);
  expect(fakes.upsert).not.toHaveBeenCalled();
});

test("an update failure observed from daemon state creates no toast", async () => {
  fakes.update.status = "error";
  fakes.update.toastOpen = false;
  await render(<UpdateProgressLayer />);
  expect(fakes.upsert).not.toHaveBeenCalled();
});

test("blocking update progress remains a persistent progress surface", async () => {
  fakes.update.blocking = true;
  fakes.update.status = "applying";
  await render(<UpdateProgressLayer />);
  expect(fakes.upsert).not.toHaveBeenCalled();
  expect(host.childElementCount).toBe(1);
});

function PluginSource() { const { activities } = usePluginActivity(); return <output>{activities.size}</output>; }
test.each(["install", "transcribe"])("%s activity stays on its own surface without transcription toasts", async (kind) => {
  await render(<PluginSource />);
  await act(async () => fakes.listeners.get("plugin-activity")?.({ payload: { id: "operation", pluginId: "plugin", kind, label: "opaque progress", status: "running", startedAt: 1, updatedAt: 2 } }));
  expect(host.querySelector("output")?.textContent).toBe("1");
  await act(async () => fakes.listeners.get("plugin-activity")?.({ payload: { id: "operation", pluginId: "plugin", kind, label: "opaque completion", status: "done", startedAt: 1, updatedAt: 3 } }));
  expect(host.querySelector("output")?.textContent).toBe("0");
  await act(async () => fakes.listeners.get("plugin-activity")?.({ payload: { id: "operation", pluginId: "plugin", kind, label: "opaque failure", status: "error", startedAt: 1, updatedAt: 4 } }));
  expect(fakes.upsert).not.toHaveBeenCalled();
  expect(fakes.add).not.toHaveBeenCalled();
});

test("secret backend updates its selected state without a duplicate confirmation toast", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => ({ ok: true, json: async () => ({ backend: "mock", status: { status: "locked" } }) })));
  const update = vi.fn();
  await render(<SecretsTab config={{ integrations: { secrets: { backend: "local" } } } as AppConfig} onUpdate={update} />);
  const select = host.querySelector<HTMLSelectElement>('[data-testid="settings-secrets-backend-select"]')!;
  expect(select).toBeTruthy();
  await act(async () => { select.value = "mock"; select.dispatchEvent(new Event("change", { bubbles: true })); });
  expect(update).toHaveBeenCalledTimes(1);
  expect(update).toHaveBeenCalledWith("integrations", { secrets: expect.objectContaining({ backend: "mock" }) });
  expect(fakes.add).not.toHaveBeenCalled();
  expect(fakes.upsert).not.toHaveBeenCalled();
});

test("failed update Retry reaches the installer once", async () => {
  fakes.update.status = "error";
  fakes.update.error = "opaque blocker";
  await render(<UpdateProgressLayer />);
  await showCapturedToast();
  const retry = [...host.querySelectorAll("button")].find(button => button.textContent === "Retry");
  expect(retry).toBeDefined();
  await act(async () => retry!.click());
  expect(fakes.update.applyUpdate).toHaveBeenCalledTimes(1);
});
