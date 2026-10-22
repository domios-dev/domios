import { afterEach, expect, it, vi } from "vitest";
import { connectNoticeToasts, createNoticePublisher, publishNotice } from "./notices";

afterEach(() => vi.restoreAllMocks());

it("publishes local notices with their actions into the toast port", async () => {
  const upsertToast = vi.fn();
  const run = vi.fn();
  createNoticePublisher({ upsertToast })({ kind: "toast", severity: "Warning", title: "opaque", detail: "detail", tabId: "tab", workspaceId: "workspace", lifetimeMs: 1000, actions: [{ label: "Retry", run }] });
  const toast = upsertToast.mock.calls[0][0];
  expect(toast.title).toBe("opaque");
  expect(toast.body).toBe("detail");
  expect(toast.tab_id).toBe("tab");
  expect(toast.workspace_id).toBe("workspace");
  expect(toast.lifetime_ms).toBe(1000);
  expect(toast.actions).toBeUndefined();
  await toast.local_actions[0].run();
  expect(run).toHaveBeenCalledOnce();
});

it("does not publish diagnostic notices into the toast stack or release console", () => {
  const upsertToast = vi.fn();
  const debug = vi.fn();
  const consoleDebug = vi.spyOn(console, "debug");
  createNoticePublisher({ upsertToast, debug })({ kind: "debug", severity: "Info", title: "diagnostic" });
  expect(upsertToast).not.toHaveBeenCalled();
  expect(debug).not.toHaveBeenCalled();
  expect(consoleDebug).not.toHaveBeenCalled();
});

it("delivers notices raised before the stack connects and disconnects ownership", () => {
  publishNotice({ kind: "toast", severity: "Info", title: "before", id: "before" });
  const port = vi.fn();
  const disconnect = connectNoticeToasts(port);
  expect(port.mock.calls[port.mock.calls.length - 1]?.[0].id).toBe("before");
  disconnect();
  port.mockClear();
  publishNotice({ kind: "toast", severity: "Info", title: "after", id: "after" });
  expect(port).not.toHaveBeenCalled();
  connectNoticeToasts(() => {})();
});

it("routes diagnostics to the debug port in development", async () => {
  vi.resetModules();
  vi.doMock("./devBuild", () => ({ DEV_BUILD: true }));
  try {
    const { createNoticePublisher: publisher } = await import("./notices");
    const upsertToast = vi.fn();
    const debug = vi.fn();
    publisher({ upsertToast, debug })({ kind: "debug", severity: "Info", title: "diagnostic" });
    expect(debug).toHaveBeenCalledOnce();
    expect(upsertToast).not.toHaveBeenCalled();
  } finally { vi.doUnmock("./devBuild"); }
});
