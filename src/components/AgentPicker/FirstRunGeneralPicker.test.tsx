import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import type { ProviderDescriptor } from "../../lib/types";
import { FirstRunGeneralPicker } from "./FirstRunGeneralPicker";

vi.mock("../../lib/bridge", () => ({
  invoke: vi.fn(async () => undefined),
  listen: vi.fn(async () => () => {}),
}));

import { invoke } from "../../lib/bridge";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

function provider(id: string, transport: ProviderDescriptor["transport"], displayName = id): ProviderDescriptor {
  return {
    schema_version: 1,
    id,
    display_name: displayName,
    transport,
    agent_runtime: transport === "chat_backend" ? {} : undefined,
    availability: "available",
    system_prompt_delivery: { cli_flag: "--system" },
    available_models: [],
    settings_schema: [],
    notifications_schema: [],
    supports_lazy_skills: false,
    has_hooks: false,
  };
}

it("Welcome offers Codex's runtime and confirms the selected provider once", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<FirstRunGeneralPicker providers={[
      provider("terminal-agent", "pty"),
      provider("codex", "chat_backend", "Codex"),
    ]} />));
    const buttons = () => Array.from(host.querySelectorAll("button"));
    const codex = buttons().find((button) => button.textContent?.includes("Codex"));
    expect(codex).toBeDefined();
    await act(async () => codex!.click());
    expect(codex!.getAttribute("aria-pressed")).toBe("true");
    const confirm = buttons().find((button) => button.textContent?.trim() === "Start General agent");
    expect(confirm?.disabled).toBe(false);
    await act(async () => confirm!.click());
    expect(invoke).toHaveBeenCalledExactlyOnceWith("complete_general_agent_first_run", {
      providerId: "codex", fullAccess: true,
    });
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
