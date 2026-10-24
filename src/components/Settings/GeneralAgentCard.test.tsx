import { afterEach, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot } from "react-dom/client";
import type { AppConfig } from "../../lib/types";
import { GeneralAgentCard } from "./GeneralAgentCard";

vi.mock("../../hooks/useAgentProviders", () => ({ useAgentProviders: () => ({ providers: [] }) }));

const roots: ReturnType<typeof createRoot>[] = [];
afterEach(async () => {
  for (const root of roots.splice(0)) await act(async () => root.unmount());
  document.body.replaceChildren();
});

it("exposes the hints switch and persists one config update per click", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  roots.push(root);
  const onUpdate = vi.fn();
  let general = { enabled: false };
  const render = async () => act(async () => {
    root.render(<GeneralAgentCard config={{ general_agent: general } as AppConfig} onUpdate={onUpdate} />);
  });
  await render();
  const control = host.querySelector<HTMLButtonElement>('[role="switch"][aria-label="Suggest Domios features I have when relevant"]');
  expect(control).not.toBeNull();
  expect(control!.getAttribute("aria-checked")).toBe("true");
  await act(async () => control!.click());
  expect(onUpdate.mock.calls).toEqual([["general_agent", { capability_hints: false }]]);
  general = { ...general, ...onUpdate.mock.calls[0][1] };
  await render();
  expect(control!.getAttribute("aria-checked")).toBe("false");
});
