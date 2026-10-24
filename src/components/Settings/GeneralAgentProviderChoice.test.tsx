import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { GeneralAgentProviderChoice } from "./GeneralAgentProviderChoice";

vi.mock("../../hooks/useAgentProviders", () => ({ useAgentProviders: () => ({ providers: [
  { id: "first", display_name: "First", transport: "pty", availability: "available" },
  { id: "second", display_name: "Second", transport: "pty", availability: "available" },
] }) }));

it("both settings entrypoints can reuse a choice that cancels without writes and confirms once", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  let finish!: () => void;
  const pending = new Promise<void>((resolve) => { finish = resolve; });
  const change = vi.fn(() => pending);
  const accessibleText = (node: Node): string => {
    if (node instanceof HTMLElement && node.getAttribute("aria-hidden") === "true") return "";
    if (node.nodeType === Node.TEXT_NODE) return node.textContent ?? "";
    return [...node.childNodes].map(accessibleText).join(" ");
  };
  const clickButton = async (name: string) => act(async () => {
    const button = [...host.querySelectorAll<HTMLButtonElement>("button")]
      .find((candidate) => (candidate.getAttribute("aria-label") ?? accessibleText(candidate).replace(/\s+/g, " ").trim()) === name);
    if (!button) throw new Error(`No button named "${name}"`);
    button.click();
  });
  try {
    await act(async () => root.render(<GeneralAgentProviderChoice current="first" enabled onSwitch={change} />));
    await clickButton("Second");
    expect(change).not.toHaveBeenCalled();
    await clickButton("Cancel");
    expect(change).not.toHaveBeenCalled();
    expect(host.querySelector('[role="dialog"]')).toBeNull();
    await clickButton("Second");
    await clickButton("Change and restart");
    await clickButton("Applying…");
    expect(change).toHaveBeenCalledExactlyOnceWith("second");
    await act(async () => finish());
    expect(host.querySelector('[role="dialog"]')).toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
