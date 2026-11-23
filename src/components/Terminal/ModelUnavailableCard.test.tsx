import "./ChatComposer.composition.harness";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { Tab } from "../../lib/types";
import { __setTabModelApiForTest } from "../Sidebar/tabModelApi";
import { ModelUnavailableCard } from "./ModelUnavailableCard";

const { launchProviderUpdate, cliStatus } = vi.hoisted(() => ({
  launchProviderUpdate: vi.fn(async () => undefined),
  cliStatus: { required: true, installed: true, display_name: "Fixture", latest_version: "1.2.0", updater: {} },
}));
vi.mock("../../lib/providerUpdate", () => ({ launchProviderUpdate }));
vi.mock("../../hooks/useProviderCliStatus", () => ({ useProviderCliStatus: () => ({ status: cliStatus, refresh: async () => undefined }) }));
vi.mock("../../lib/useAgentDescriptor", () => ({
  useAgentDescriptor: () => ({
    id: "fixture",
    display_name: "Fixture",
    model_switch: "next_turn",
    available_models: [{ id: "refused", display_name: "Refused" }, { id: "alternate", display_name: "Alternate" }],
  }),
}));

it("opens the live model picker and offers the declared CLI update", async () => {
  const tab = {id: "tab", agent_type: "fixture", backing: {kind: "chat_backend"}, agent_requested_model: "refused"} as Tab;
  const switchModel = vi.fn(async () => ({mode_used:"next_turn" as const, model_confirmation:"pending" as const}));
  __setTabModelApiForTest({switchModel});
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<ModelUnavailableCard tab={tab} model="refused" />));
    const card = host.querySelector('section[aria-label="Model unavailable"]')!;
    expect([...card.querySelectorAll("button")].some(button => /continue/i.test(button.getAttribute("aria-label") ?? button.textContent ?? ""))).toBe(false);
    await act(async () => card.querySelector<HTMLButtonElement>('button[aria-label="Choose another model"]')!.click());
    const dialog = document.body.querySelector('[role="dialog"]')!;
    const alternate = [...dialog.querySelectorAll<HTMLElement>('[role="option"]:not([aria-disabled="true"])')];
    expect(alternate).toHaveLength(1);
    await act(async () => alternate[0].click());
    await act(async () => [...dialog.querySelectorAll("button")].find(button => button.textContent === "Apply")!.click());
    expect(switchModel).toHaveBeenCalledWith("tab", "alternate", undefined);
    const update = card.querySelector('[role="group"][aria-label="Provider CLI update"]')!;
    await act(async () => [...update.querySelectorAll("button")].find(button => button.textContent === "Update")!.click());
    expect(launchProviderUpdate).toHaveBeenCalledWith("fixture", "tab");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    __setTabModelApiForTest(null);
  }
});
