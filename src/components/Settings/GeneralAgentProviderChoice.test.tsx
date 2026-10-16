import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { GeneralAgentProviderChoice } from "./GeneralAgentProviderChoice";

vi.mock("../../hooks/useAgentProviders", () => ({ useAgentProviders: () => ({ providers: [
  { id: "first", display_name: "First" }, { id: "second", display_name: "Second" },
] }) }));

it("both settings entrypoints can reuse a choice that cancels without writes and confirms once", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let finish!: () => void;
  const pending = new Promise<void>((resolve) => { finish = resolve; });
  const change = vi.fn(() => pending);
  const click = async (selector: string) => act(async () => host.querySelector<HTMLButtonElement>(selector)!.click());
  try {
    await act(async () => root.render(<GeneralAgentProviderChoice current="first" enabled onSwitch={change} />));
    await click('[data-testid="agent-picker-card-second"]');
    expect(change).not.toHaveBeenCalled();
    await click('[data-testid="confirm-dialog-cancel"]');
    expect(change).not.toHaveBeenCalled();
    expect(host.querySelector('[role="dialog"]')).toBeNull();
    await click('[data-testid="agent-picker-card-second"]');
    await click('[data-testid="confirm-dialog-confirm"]');
    await click('[data-testid="confirm-dialog-confirm"]');
    expect(change).toHaveBeenCalledExactlyOnceWith("second");
    await act(async () => finish());
    expect(host.querySelector('[role="dialog"]')).toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
