import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CommandPalette, type CommandItem } from "./CommandPalette";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  root = null;
  document.body.innerHTML = "";
  vi.useRealTimers();
});

async function openPalette(commands: CommandItem[]) {
  vi.useFakeTimers();
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<CommandPalette commands={commands} />));
  await act(async () => {
    window.dispatchEvent(new KeyboardEvent("keydown", { code: "KeyK", ctrlKey: true, metaKey: true, bubbles: true }));
  });
  const dialog = document.querySelector<HTMLElement>('[role="dialog"]');
  const input = document.querySelector<HTMLInputElement>('input[aria-label="Search commands and Tabs"]');
  if (!dialog || !input) throw new Error("palette did not open as a labelled dialog");
  return { dialog, input };
}

async function type(input: HTMLInputElement, value: string) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

describe("command palette dialog", () => {
  it("opens as a labelled modal dialog and says when nothing matches", async () => {
    const { dialog, input } = await openPalette([{ id: "x", label: "New Tab", action: () => {} }]);
    expect(dialog.getAttribute("aria-modal")).toBe("true");
    expect(dialog.getAttribute("aria-label")).toBe("Command palette");
    await type(input, "nothing like this");
    expect(document.querySelector('[role="status"]')?.textContent).toContain("nothing like this");
    expect(document.querySelectorAll('[role="option"]')).toHaveLength(0);
  });

  it("Enter runs the highlighted option even when sections interleave", async () => {
    const ran: string[] = [];
    const { input } = await openPalette([
      { id: "a1", label: "Alpha", section: "App", action: () => ran.push("a1") },
      { id: "b1", label: "Beta", section: "Agents", action: () => ran.push("b1") },
      { id: "a2", label: "Gamma", section: "App", action: () => ran.push("a2") },
    ]);
    await act(async () => {
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    });
    const selected = document.querySelector('[role="option"][aria-selected="true"]');
    expect(selected?.textContent).toContain("Gamma");
    await act(async () => {
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      vi.runAllTimers();
    });
    expect(ran).toEqual(["a2"]);
  });
});
