import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { useInstallHintLayout } from "./useInstallHintLayout";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
afterEach(() => { document.body.replaceChildren(); vi.unstubAllGlobals(); });

it("publishes measured bounds and updates on composer resize, then unsubscribes", async () => {
  let deliver: () => void = () => {};
  const disconnect = vi.fn();
  vi.stubGlobal("ResizeObserver", class {
    constructor(callback: () => void) { deliver = callback; }
    observe() {}
    unobserve() {}
    disconnect = disconnect;
  });
  const titlebar = document.createElement("div");
  titlebar.dataset.testid = "titlebar";
  titlebar.getBoundingClientRect = () => ({ bottom: 44 } as DOMRect);
  const composer = document.createElement("div");
  composer.setAttribute("data-composer-height-surface", "");
  let composerTop = 700;
  composer.getBoundingClientRect = () => ({ top: composerTop, height: 100, width: 390 } as DOMRect);
  const host = document.createElement("div");
  document.body.append(titlebar, composer, host);
  function Probe() {
    const layout = useInstallHintLayout(true, true);
    return <output aria-label="Hint bounds">{JSON.stringify(layout)}</output>;
  }
  const root = createRoot(host);
  await act(async () => root.render(<Probe />));
  expect(JSON.parse(host.textContent!)).toMatchObject({ top: 50, maxHeight: 644 });
  composerTop = 280;
  await act(async () => deliver());
  expect(JSON.parse(host.textContent!)).toMatchObject({ top: 50, maxHeight: 224 });
  await act(async () => root.unmount());
  expect(disconnect).toHaveBeenCalledOnce();
});
