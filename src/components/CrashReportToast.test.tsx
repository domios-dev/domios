import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { AppConfig, Notification } from "../lib/types";
import { RecoveryStatusBanner } from "./CrashReportToast";
import { UpdateProgressLayer } from "./UpdateProgressLayer";
import { InternalToast } from "./Toast/InternalToast";
import { SecretsTab } from "./Settings/SecretsTab";
import { usePluginActivity } from "../lib/usePluginActivity";
import { useBlueprintRunPopups } from "./Automation/useBlueprintRunPopups";
import { NotificationToastContainer } from "./Notifications/NotificationToast";
import { NotificationPanel } from "./Notifications/NotificationPanel";

const fakes = vi.hoisted(() => ({
  upsert: vi.fn(), add: vi.fn(), dismiss: vi.fn(), invoke: vi.fn(),
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
  update: { status: "up-to-date", latestVersion: "2", error: null, toastOpen: true, blocking: false, dismissUpdateToast: vi.fn() },
  graphChanged: undefined as (() => void) | undefined,
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
vi.mock("./Automation/resource", () => ({ AutomationResource: class {
  subscribe(handler: () => void) { fakes.graphChanged = handler; return () => { fakes.graphChanged = undefined; }; }
  getSnapshot() { return { graph: {}, failed: false }; }
} }));
vi.mock("./Automation/runPopupModel", () => ({
  emptyPopupState: () => ({}),
  advanceRunPopups: () => ({ state: {}, toasts: [{ id: "run-source", title: "opaque run", body: null, tab_id: null, workspace_id: "workspace", severity: "Info" }] }),
}));

let host: HTMLDivElement;
let root: Root;
const recoveryState = (settled = false) => ({
  current: { epoch: "new", pid: 42, started_at: 1, last_healthy_at: 2 },
  crash_presentation: settled ? "settled" : "active",
  last_crash: {
    classification: "abnormal_exit", detected_at: 3, new_epoch: "new",
    previous: { epoch: "previous", pid: 41, last_healthy_at: 1 },
    supervisor_action: { backoff_ms: 1000, loop_protected: false, crash_count_in_window: 1 },
    work_policy: { ambiguous_execution: { missing_bindings: 2, disposition: "review" }, duplicate_worker_guard: "guarded", report_route_guard: "guarded" },
  },
});

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  localStorage.clear();
  fakes.listeners.clear();
  fakes.invoke.mockImplementation(async (command: string) => command === "get_recovery_state" ? recoveryState() : {});
  fakes.update.status = "up-to-date";
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

test("crash recovery routes through the host and keeps report actions in the bell", async () => {
  await render(<RecoveryStatusBanner />);
  expect(fakes.upsert).toHaveBeenCalledTimes(1);
  await act(async () => fakes.listeners.get("crash_report_available")?.({ payload: { dir: "reports", files: ["report.txt"], recovery: recoveryState() } }));
  const value = await showCapturedToast();
  expect(value.body).toContain("2 bindings not restored");
  await render(<NotificationPanel notifications={[{ ...value, timestamp: 1, read: false }]} onMarkRead={() => {}} onMarkAllRead={() => {}} onDelete={() => {}} onClearAll={() => {}} onJumpToTab={() => {}} onClose={() => {}} />);
  const action = [...host.querySelectorAll("button")].find((button) => button.textContent === "Open folder");
  expect(action).toBeDefined();
  await act(async () => action!.click());
  expect(fakes.invoke).toHaveBeenCalledWith("open_folder", { path: "reports" });
});

test("settled crash history does not create a toast", async () => {
  fakes.invoke.mockResolvedValue(recoveryState(true));
  await render(<RecoveryStatusBanner />);
  expect(fakes.upsert).not.toHaveBeenCalled();
});

test("the same crash is not replayed after frontend remount", async () => {
  await render(<RecoveryStatusBanner />);
  expect(fakes.upsert).toHaveBeenCalledTimes(1);
  await render(null);
  await render(<RecoveryStatusBanner />);
  expect(fakes.upsert).toHaveBeenCalledTimes(1);
});

test("new settled event wins over a stale initial recovery read", async () => {
  let resolve!: (value: unknown) => void;
  fakes.invoke.mockImplementation(() => new Promise((done) => { resolve = done; }));
  await render(<RecoveryStatusBanner />);
  await act(async () => fakes.listeners.get("recovery-state-changed")?.({ payload: recoveryState(true) }));
  await act(async () => resolve(recoveryState()));
  expect(fakes.upsert).not.toHaveBeenCalled();
});

test("internal prompt notices render through the host once", async () => {
  const source = <InternalToast pendingPrompt={{ question: "opaque prompt" }} tabId="tab" agentType="provider" />;
  await render(source);
  await render(source);
  expect(fakes.upsert).toHaveBeenCalledTimes(1);
  const value = await showCapturedToast();
  expect(value.tab_id).toBe("tab");
});

test("update transient notices render through the host", async () => {
  await render(<UpdateProgressLayer />);
  expect(fakes.update.dismissUpdateToast).toHaveBeenCalledTimes(1);
  await showCapturedToast();
});

test("blocking update progress remains a persistent progress surface", async () => {
  fakes.update.blocking = true;
  fakes.update.status = "applying";
  await render(<UpdateProgressLayer />);
  expect(fakes.upsert).not.toHaveBeenCalled();
  expect(host.childElementCount).toBe(1);
});

function PluginSource() { const { activities } = usePluginActivity(); return <output>{activities.size}</output>; }
test("plugin activity events update their own surface without creating notifications", async () => {
  await render(<PluginSource />);
  await act(async () => fakes.listeners.get("plugin-activity")?.({ payload: { id: "operation", pluginId: "plugin", kind: "install", label: "opaque progress", status: "running", startedAt: 1, updatedAt: 2 } }));
  expect(host.querySelector("output")?.textContent).toBe("1");
  await act(async () => fakes.listeners.get("plugin-activity")?.({ payload: { id: "operation", pluginId: "plugin", kind: "install", label: "opaque completion", status: "done", startedAt: 1, updatedAt: 3 } }));
  expect(host.querySelector("output")?.textContent).toBe("0");
  expect(fakes.upsert).not.toHaveBeenCalled();
  expect(fakes.add).not.toHaveBeenCalled();
});

function RunSource() { useBlueprintRunPopups([{ id: "workspace" }], 1, [], fakes.upsert); return null; }
test("blueprint run updates render through the host", async () => {
  await render(<RunSource />);
  await act(async () => fakes.graphChanged?.());
  const value = await showCapturedToast();
  expect(value.id).toBe("run-source");
});

test("secret backend confirmation renders through the host", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => ({ ok: true, json: async () => ({ backend: "mock", status: { status: "locked" } }) })));
  await render(<SecretsTab config={{ integrations: { secrets: { backend: "local" } } } as AppConfig} onUpdate={() => {}} />);
  const select = host.querySelector<HTMLSelectElement>('[data-testid="settings-secrets-backend-select"]')!;
  expect(select).toBeTruthy();
  await act(async () => { select.value = "mock"; select.dispatchEvent(new Event("change", { bubbles: true })); });
  await showCapturedToast("add");
});
