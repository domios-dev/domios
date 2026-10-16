import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { setAgentDescriptors } from "../../lib/agentDescriptorCache";
import type { AppConfig, ProviderDescriptor } from "../../lib/types";
import { SchemaRenderer } from "./SchemaRenderer";

afterEach(() => {
  setAgentDescriptors([]);
  document.body.replaceChildren();
});

it("settings use the live chat catalogue and save the selected default only on Apply", async () => {
  const provider = {
    id: "fixture", display_name: "Fixture", available_models: [
      { id: "fast", display_name: "Fast", group: "Available", primary: true, reasoning_efforts: [] },
      { id: "blocked", display_name: "Blocked", group: "Available", primary: true, availability: "unavailable", reasoning_efforts: [] },
    ],
  } as unknown as ProviderDescriptor;
  setAgentDescriptors([provider]);
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const save = vi.fn();
  await act(async () => root.render(<SchemaRenderer providerId="fixture" config={{ agents: {} } as AppConfig}
    schema={[{ kind: "select", key: "default_model", label: "Default model", options: [["stale", "Stale schema choice"]] }]}
    onChange={save} />));
  const trigger = host.querySelector<HTMLButtonElement>('[aria-label="Choose default model"]')!;
  await act(async () => trigger.click());
  const options = [...document.body.querySelectorAll<HTMLButtonElement>('[role="option"]')];
  expect(options.map((option) => option.textContent)).toEqual(["Fast", "Blocked"]);
  expect(options[1].getAttribute("aria-disabled")).toBe("true");
  await act(async () => options[0].click());
  expect(save).not.toHaveBeenCalled();
  const apply = [...document.body.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "Apply")!;
  await act(async () => apply.click());
  expect(save).toHaveBeenCalledExactlyOnceWith("fixture", { default_model: "fast" });
  expect(document.body.querySelector('[role="dialog"]')).toBeNull();
  expect(document.activeElement).toBe(trigger);
  await act(async () => root.unmount());
});
