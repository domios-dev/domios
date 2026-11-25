import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { ProviderUpdateBubble, ProviderUpdateNotice } from "./ProviderUpdateBubble";

const mocked = vi.hoisted(() => ({
  refresh: vi.fn(async () => undefined),
  useProviderCliStatus: vi.fn(),
  launchProviderUpdate: vi.fn(async (..._args: unknown[]) => ({ tab_id: "cli-tab", launch_state: "command_dispatched" })),
}));
vi.mock("../../hooks/useProviderCliStatus", () => ({
  useProviderCliStatus: (...args: unknown[]) => {
    mocked.useProviderCliStatus(...args);
    return {
      status: { required: true, installed: true, latest_version: "1.2.3", updater: "opaque --update", display_name: "fixture" },
      refresh: mocked.refresh,
    };
  },
}));
vi.mock("../../lib/providerUpdate", () => ({
  launchProviderUpdate: mocked.launchProviderUpdate,
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const host = document.createElement("div");
document.body.appendChild(host);
const root = createRoot(host);
afterEach(async () => {
  await act(async () => root.render(null));
  mocked.refresh.mockClear();
  mocked.useProviderCliStatus.mockClear();
  mocked.launchProviderUpdate.mockReset().mockResolvedValue({ tab_id: "cli-tab", launch_state: "command_dispatched" });
});

it("offers update and dismissal and displays the model result in the same status", async () => {
  const update = vi.fn(), dismiss = vi.fn();
  const model = { key: "1.2.3", state: "available" as const, message: "opaque offer", canUpdate: true };
  await act(async () => root.render(<ProviderUpdateNotice model={model} onUpdate={update} onDismiss={dismiss} />));
  const buttons = [...host.querySelectorAll("button")];
  const action = buttons.find(button => button.textContent === "Update");
  expect(action).toBeDefined();
  await act(async () => action!.click());
  expect(update).toHaveBeenCalledTimes(1);
  await act(async () => host.querySelector<HTMLButtonElement>('button[aria-label="Dismiss provider update"]')!.click());
  expect(dismiss).toHaveBeenCalledTimes(1);
  await act(async () => root.render(<ProviderUpdateNotice model={{ ...model, state: "succeeded", message: "opaque result", canUpdate: false }} onUpdate={update} onDismiss={dismiss} />));
  expect(host.querySelector('[role="status"]')?.textContent).toBe("opaque result");
  expect([...host.querySelectorAll("button")].some(button => button.textContent === "Update")).toBe(false);
});

it("places provider updates before and outside the complete composer group", async () => {
  const { ChatComposerSurface } = await import("./ChatComposerSurface");
  await act(async () => root.render(<ChatComposerSurface view={{kind:"none"}} providerId="fixture" tabId="parent"><button type="button">Plan fixture</button><textarea aria-label="Draft" /></ChatComposerSurface>));
  const notice=host.querySelector('[role="group"][aria-label="Provider CLI update"]')!;
  const composer=host.querySelector('[role="group"][aria-label="Message composer"]')!;
  expect(notice).not.toBeNull();
  expect(composer.contains(notice)).toBe(false);
  expect(Boolean(notice.compareDocumentPosition(composer) & Node.DOCUMENT_POSITION_FOLLOWING)).toBe(true);
  expect(composer.querySelector('textarea[aria-label="Draft"]')).not.toBeNull();
});

it("omits an update action with an empty notice", async () => {
  await act(async () => root.render(<ProviderUpdateNotice model={{ key: "job", state: "failed", message: "  ", canUpdate: true }} onUpdate={vi.fn()} onDismiss={vi.fn()} />));
  expect(host.childElementCount).toBe(0);
});

it("shows the provider notice after settlement and preserves dismissal through the next turn", async () => {
  const { ChatComposerSurface } = await import("./ChatComposerSurface");
  const notice = () => host.querySelector('[role="group"][aria-label="Provider CLI update"]');
  const render = (turnInProgress: boolean) => act(async () => root.render(
    <ChatComposerSurface view={{ kind: "none" }} providerId="fixture" turnInProgress={turnInProgress}>
      <textarea aria-label="Draft" />
    </ChatComposerSurface>,
  ));
  await render(true);
  expect(notice()).toBeNull();
  await render(false);
  expect(notice()).not.toBeNull();
  await act(async () => notice()?.querySelector<HTMLButtonElement>('button[aria-label="Dismiss provider update"]')?.click());
  await render(true);
  await render(false);
  expect(notice()).toBeNull();
});

it("targets the remote peer and its own tab id for a composite mesh tab and surfaces errors", async () => {
  await act(async () => root.render(<ProviderUpdateBubble providerId="fixture" callerTabId="mesh:blackstar/remote-tab-9" />));
  expect(mocked.useProviderCliStatus).toHaveBeenLastCalledWith("fixture", "blackstar");
  const updateButton = () => [...host.querySelectorAll("button")].find(button => button.textContent === "Update")!;
  await act(async () => updateButton().click());
  expect(mocked.launchProviderUpdate).toHaveBeenCalledExactlyOnceWith("fixture", "remote-tab-9", "blackstar");
  expect(mocked.refresh).toHaveBeenCalledTimes(1);

  mocked.launchProviderUpdate.mockRejectedValueOnce(new Error("Remote updater refused"));
  await act(async () => updateButton().click());
  expect(host.querySelector('[role="status"]')?.textContent).toBe("Remote updater refused");

  await act(async () => root.render(<ProviderUpdateBubble providerId="fixture" callerTabId="mesh:other-peer/tab-2" />));
  expect(mocked.useProviderCliStatus).toHaveBeenLastCalledWith("fixture", "other-peer");
  expect(host.querySelector('[role="status"]')?.textContent).not.toBe("Remote updater refused");
});
