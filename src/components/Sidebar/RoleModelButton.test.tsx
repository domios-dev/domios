import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderDescriptor } from "../../lib/types";
import { RoleModelButton } from "./RoleModelButton";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

// Speed and mode would land under Advanced in a running tab's picker.
const provider = {
  id: "vendor",
  display_name: "Vendor",
  model_switch: "next_turn",
  available_models: [
    { id: "fast", display_name: "Fast", reasoning_efforts: [] },
    { id: "deep", display_name: "Deep", reasoning_efforts: [{ id: "low", display_name: "Low" }, { id: "high", display_name: "High" }], default_reasoning_effort: "high" },
  ],
  agent_runtime: {
    runtimeModes: [{ id: "full-access", displayName: "Full access" }],
    serviceTiers: [{ id: "priority", name: "Fast", description: "" }],
  },
} as unknown as ProviderDescriptor;

let root: Root | null = null;
let host: HTMLDivElement;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  root = null;
  host.remove();
});

describe("a role's model pill", () => {
  it("opens the unified picker with effort but no Advanced section, and commits model and effort", async () => {
    const onChange = vi.fn();
    await act(async () => {
      root!.render(<RoleModelButton label="Planner" provider={provider} modelId="deep" effortId="high" onChange={onChange} testId="pill" />);
    });
    await act(async () => host.querySelector<HTMLButtonElement>('[data-testid="pill"]')!.click());
    const picker = document.body.querySelector<HTMLElement>('[data-testid="status-model-picker"]')!;
    expect(picker).not.toBeNull();
    expect(picker.querySelector('[aria-label="Reasoning"]')).not.toBeNull();
    expect(picker.querySelector("details")).toBeNull();
    expect(picker.textContent).not.toContain("Advanced");
    const fast = Array.from(picker.querySelectorAll<HTMLElement>('[role="option"]')).find((option) => option.textContent?.startsWith("Fast"))!;
    await act(async () => fast.click());
    const apply = Array.from(picker.querySelectorAll<HTMLButtonElement>("button")).find((button) => button.textContent?.trim() === "Apply")!;
    await act(async () => apply.click());
    expect(onChange).toHaveBeenCalledOnce();
    expect(onChange.mock.calls[0][0]).toBe("fast");
  });
});
