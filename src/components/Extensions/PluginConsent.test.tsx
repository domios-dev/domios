import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { PluginConsentDialog } from "./PluginConsentDialog";
import { PluginView } from "./PluginView";
import { GlancePopover } from "./GlancePopover";
import { PluginPermissions } from "./PluginPermissions";
import { requestPluginConsent, getPluginConsent, decidePluginConsent } from "../../lib/pluginConsentStore";
import { invokePluginWithConsent } from "../../lib/pluginConsent";
import { __resetPluginSnapshotStoreForTest, runPluginSnapshotAction } from "../../lib/pluginSnapshotStore";
import type { PluginSnapshot } from "./types";
import { invoke } from "../../lib/bridge";

vi.mock("../../lib/bridge", () => ({ invoke: vi.fn(), listen: vi.fn(async () => () => {}) }));
vi.mock("./usePluginDocument", () => ({ usePluginDocument: () => ({ html: "<html></html>", error: null }) }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let root: Root | undefined;
let host: HTMLElement;
const permission = { key: "local_recording", label: "Record your selected microphone", allowed: false, decided: false };
const snapshot = (): PluginSnapshot => ({ plugins: [{ id: "fixture", display_name: "Fixture", sensitive_permissions: [{ ...permission }] }], channels: [], diff: [] });

async function mount(content = <PluginConsentDialog />) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(content));
}
function button(name: string) { return [...document.querySelectorAll<HTMLButtonElement>("button")].find((item) => item.textContent === name)!; }
afterEach(async () => {
  await act(async () => { while (getPluginConsent()) decidePluginConsent(false); root?.unmount(); });
  host?.remove();
  root = undefined;
  __resetPluginSnapshotStoreForTest();
  vi.resetAllMocks();
});

it("dialog Allow and Not now settle one decision through shared buttons", async () => {
  await mount();
  let result: Promise<boolean>;
  await act(async () => { result = requestPluginConsent({ pluginId: "fixture", pluginName: "Fixture", permission: permission.key, label: permission.label }); });
  expect(document.querySelector('[role="dialog"][aria-modal="true"]')).not.toBeNull();
  await act(async () => button("Allow").click());
  expect(await result!).toBe(true);
  await act(async () => { result = requestPluginConsent({ pluginId: "fixture", pluginName: "Fixture", permission: permission.key, label: permission.label }); });
  await act(async () => button("Not now").click());
  expect(await result!).toBe(false);
});

it("permissions toggle persists once and shows the authoritative state", async () => {
  vi.mocked(invoke).mockResolvedValue(snapshot());
  await mount(<PluginPermissions pluginId="fixture" permissions={[permission]} />);
  const toggle = document.querySelector<HTMLButtonElement>('[role="switch"]')!;
  expect(toggle.getAttribute("aria-checked")).toBe("false");
  await act(async () => toggle.click());
  expect(invoke).toHaveBeenCalledExactlyOnceWith("plugin_manager_action", { action: { kind: "permission", pluginId: "fixture", permission: "local_recording", allowed: true } });
  await act(async () => root!.render(<PluginPermissions pluginId="fixture" permissions={[{ ...permission, allowed: true }]} />));
  expect(document.querySelector('[role="switch"]')?.getAttribute("aria-checked")).toBe("true");
});

it("host invocation waits for Allow, records grant and retries original args once", async () => {
  const args = { pluginId: "fixture", method: "enable", args: { enabled: true } };
  const calls: string[] = [];
  vi.mocked(invoke).mockImplementation(async (command) => {
    calls.push(command);
    if (command === "plugin_manager_snapshot" || command === "plugin_manager_action") return snapshot();
    if (calls.filter((item) => item === "plugin_view_call").length === 1) throw new Error("local_recording_grant_required");
    return { enabled: true };
  });
  await mount();
  let result: Promise<unknown>;
  await act(async () => { result = invokePluginWithConsent("plugin_view_call", args); });
  expect(calls).toEqual(["plugin_manager_snapshot", "plugin_view_call"]);
  await act(async () => button("Allow").click());
  expect(await result!).toEqual({ enabled: true });
  expect(calls).toEqual(["plugin_manager_snapshot", "plugin_view_call", "plugin_manager_action", "plugin_view_call"]);
  expect(vi.mocked(invoke).mock.calls.find(([command]) => command === "plugin_manager_action")?.[1]).toEqual({ action: {
    kind: "permission", pluginId: "fixture", permission: "local_recording", allowed: true, permissionSource: "action",
  } });
  expect(vi.mocked(invoke).mock.calls.filter(([command]) => command === "plugin_view_call").map(([, argument]) => argument)).toEqual([args, args]);
});

it("install and update ask once per undecided permission and retain Not now", async () => {
  let current = snapshot();
  vi.mocked(invoke).mockImplementation(async (_command, args) => {
    const action = args?.action as { kind: string; allowed?: boolean };
    if (action.kind === "permission") current.plugins[0].sensitive_permissions = [{ ...permission, allowed: action.allowed!, decided: true }];
    return current;
  });
  await mount();
  let result: Promise<PluginSnapshot>;
  await act(async () => { result = runPluginSnapshotAction({ kind: "install", pluginId: "fixture" }); });
  expect(getPluginConsent()?.pluginId).toBe("fixture");
  await act(async () => button("Not now").click());
  await result!;
  expect(current.plugins[0].sensitive_permissions?.[0].allowed).toBe(false);
  expect(vi.mocked(invoke).mock.calls[1][1]).toEqual({ action: {
    kind: "permission", pluginId: "fixture", permission: "local_recording", allowed: false, permissionSource: "install",
  } });
  await act(async () => { await runPluginSnapshotAction({ kind: "update", pluginId: "fixture" }); });
  expect(getPluginConsent()).toBeNull();
  expect(invoke).toHaveBeenCalledTimes(3);
});

it("an obsolete surface never retries after consent", async () => {
  let current = true;
  const action = vi.fn().mockRejectedValue(new Error("local_recording_grant_required"));
  vi.mocked(invoke).mockImplementation(async (command) => command === "plugin_view_call" ? action() : snapshot());
  await mount();
  let result: Promise<unknown>;
  let failure: Promise<unknown>;
  await act(async () => {
    result = invokePluginWithConsent("plugin_view_call", { pluginId: "fixture", method: "enable" }, () => current);
    failure = result.catch((error: Error) => error.message);
  });
  current = false;
  await act(async () => button("Allow").click());
  expect(await failure!).toBe("Plugin action was cancelled.");
  expect(action).toHaveBeenCalledTimes(1);
});

it("iframe consent preserves the host-stamped plugin, call ID and reply channel", async () => {
  let attempts = 0;
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command !== "plugin_view_call") return snapshot();
    if (++attempts === 1) throw new Error("local_recording_grant_required");
    return { enabled: true };
  });
  await mount(<><PluginConsentDialog /><PluginView pluginId="fixture" api={undefined} embedded /></>);
  const iframe = host.querySelector("iframe")!;
  const reply = vi.fn();
  const ownWindow = { postMessage: reply };
  Object.defineProperty(iframe, "contentWindow", { configurable: true, value: ownWindow });
  await act(async () => window.dispatchEvent(new MessageEvent("message", {
    source: ownWindow as unknown as Window, data: { __ctInvoke: 7, pluginId: "foreign", method: "enable", args: { enabled: true } },
  })));
  expect(reply).not.toHaveBeenCalled();
  await act(async () => button("Allow").click());
  expect(reply).toHaveBeenCalledExactlyOnceWith({ __ctReply: 7, result: { enabled: true } }, "*");
  const calls = vi.mocked(invoke).mock.calls.filter(([command]) => command === "plugin_view_call");
  expect(calls.map(([, args]) => args?.pluginId)).toEqual(["fixture", "fixture"]);
});

it("glance action replaces a declarative grant error with consent and retries once", async () => {
  let attempts = 0;
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command !== "plugin_glance") {
      const value = snapshot();
      value.plugins[0].capabilities = { glance_view: true };
      return value;
    }
    if (!args?.action) return { nodes: [{ kind: "button", label: "Enable fixture", action: "enable" }] };
    if (++attempts === 1) return { nodes: [{ kind: "note", body: "local_recording_grant_required", level: "error" }] };
    return { nodes: [{ kind: "badge", label: "Enabled", tone: "ok" }] };
  });
  await mount(<><PluginConsentDialog /><GlancePopover pluginId="fixture" displayName="Fixture" anchor={{ right: 0, top: 0 }} onClose={() => {}} onOpenSettings={() => {}} /></>);
  await act(async () => button("Enable fixture").click());
  expect(getPluginConsent()?.permission).toBe("local_recording");
  expect(document.body.textContent).not.toContain("local_recording_grant_required");
  await act(async () => button("Allow").click());
  expect(attempts).toBe(2);
  expect(document.body.textContent).toContain("Enabled");
});

it("a stale dialog decision cannot approve the next queued permission", async () => {
  await mount();
  const first = { pluginId: "fixture", pluginName: "Fixture", permission: "local_recording", label: "Record" };
  const second = { pluginId: "fixture", pluginName: "Fixture", permission: "spawn_agents", label: "Start agents" };
  let accepted: Promise<boolean>;
  let denied: Promise<boolean>;
  await act(async () => {
    accepted = requestPluginConsent(first);
    denied = requestPluginConsent(second);
  });
  await act(async () => { decidePluginConsent(true, first); decidePluginConsent(true, first); });
  expect(await accepted!).toBe(true);
  expect(getPluginConsent()).toBe(second);
  await act(async () => button("Not now").click());
  expect(await denied!).toBe(false);
});
