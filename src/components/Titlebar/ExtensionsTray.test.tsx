import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ExtensionsTray } from "./ExtensionsTray";
import { SELECT_EXTENSION_EVENT } from "./extensionsTrayLogic";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLElement | null = null;

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  host?.remove();
  root = null;
  host = null;
});

async function render(onOpen = vi.fn()) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<ExtensionsTray onOpen={onOpen} />));
  return { host, onOpen };
}

describe("extensions titlebar entry", () => {
  it("opens the Plugins root without plugin-specific tray entries", async () => {
    const opened = vi.fn();
    const ids: Array<string | null> = [];
    const onSelect = (event: Event) => ids.push((event as CustomEvent<{ id: string | null }>).detail.id);
    window.addEventListener(SELECT_EXTENSION_EVENT, onSelect);

    const { host } = await render(opened);
    const button = host.querySelector<HTMLButtonElement>('[data-testid="titlebar-extensions"]');
    expect(button?.getAttribute("title")).toBe("Plugins");
    expect(host.querySelectorAll("button")).toHaveLength(1);
    expect(host.querySelector('[data-testid^="tray-plugin-"]')).toBeNull();

    await act(async () => button!.click());
    window.removeEventListener(SELECT_EXTENSION_EVENT, onSelect);
    expect(opened).toHaveBeenCalledTimes(1);
    expect(ids).toEqual([null]);
  });
});
