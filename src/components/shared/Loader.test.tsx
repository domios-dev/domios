import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { Loader } from "./Loader";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function render(node: React.ReactNode) {
  const host = document.createElement("div");
  const root = createRoot(host);
  await act(async () => root.render(node));
  return { host, close: () => act(async () => root.unmount()) };
}

it("is the branded loader in every area: the mark over the dot matrix, with its caption, deferred", async () => {
  const view = await render(<Loader label="Loading the conversation" detail="Reading history" data-testid="loader" />);
  try {
    const status = view.host.querySelector('[role=status][aria-label="Loading the conversation"]')!;
    expect(status.querySelector("canvas")).not.toBeNull();
    expect(status.querySelector("svg.brand-mark")).not.toBeNull();
    expect(status.textContent).toContain("Reading history");
    const box = view.host.querySelector<HTMLElement>('[data-testid="loader"]')!;
    expect(box.classList.contains("ct-deferred-loader")).toBe(true);
    // The surface fills a positioned box with a height of its own, so the dots always have room to spread.
    expect(box.className).toContain("min-h-48");
    expect(status.className).toContain("absolute");
  } finally { await view.close(); }
});

it("hands over at once where a cover is already on screen, and draws a smaller mark when compact", async () => {
  const view = await render(<><Loader label="Starting Domios" variant="fullscreen" deferred={false} data-testid="boot" />
    <Loader label="Loading transcript" variant="compact" /></>);
  try {
    const boot = view.host.querySelector<HTMLElement>('[data-testid="boot"]')!;
    expect(boot.classList.contains("ct-deferred-loader")).toBe(false);
    expect(boot.className).toContain("fixed");
    expect(view.host.querySelector('[aria-label="Loading transcript"] svg.brand-mark')?.getAttribute("width")).toBe("48");
  } finally { await view.close(); }
});
