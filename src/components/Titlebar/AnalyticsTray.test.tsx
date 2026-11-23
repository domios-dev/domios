import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AnalyticsTray } from "./AnalyticsTray";
import { ANALYTICS_PLUGIN_ID, isAnalyticsAvailable } from "./analyticsTrayLogic";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let host: HTMLElement | null = null;

async function render(open: boolean, onToggle = vi.fn()) {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(<AnalyticsTray open={open} onToggle={onToggle} />));
  return { host, onToggle };
}

afterEach(async () => {
  if (root) await act(async () => root!.unmount());
  host?.remove();
  root = null;
  host = null;
});

describe("analytics titlebar entry", () => {
  it("is available only for the installed core plugin that declares its view", () => {
    expect(isAnalyticsAvailable([{ id: ANALYTICS_PLUGIN_ID, capabilities: { view: true } }])).toBe(true);
    expect(isAnalyticsAvailable([{ id: ANALYTICS_PLUGIN_ID, capabilities: { view: false } }])).toBe(false);
    expect(isAnalyticsAvailable([{ id: ANALYTICS_PLUGIN_ID }])).toBe(false);
    expect(isAnalyticsAvailable([{ id: "git", capabilities: { view: true } }])).toBe(false);
  });

  it("is a named keyboard button that marks the open view and toggles it", async () => {
    const { host, onToggle } = await render(true);
    const button = host.querySelector<HTMLButtonElement>('[data-testid="titlebar-analytics"]');
    expect(button?.tagName).toBe("BUTTON");
    expect(button?.getAttribute("type")).toBe("button");
    expect(button?.getAttribute("aria-label")).toBe("Analytics");
    expect(button?.getAttribute("aria-current")).toBe("page");
    expect(button?.querySelector("svg")?.getAttribute("aria-hidden")).toBe("true");
    await act(async () => button!.click());
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("drops the open marker when the view is closed", async () => {
    const { host } = await render(false);
    expect(host.querySelector('[data-testid="titlebar-analytics"]')?.hasAttribute("aria-current")).toBe(false);
  });
});
