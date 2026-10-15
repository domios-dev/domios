import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
vi.mock("../../lib/devBuild", () => ({ DEV_BUILD: true }));
import { PluginUiHost } from "./PluginUiHost";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

describe("shared plugin UI host wiring", () => {
  it("shows loading, the plugin failure and retry, then ready content", async () => {
    const host = document.createElement("div");
    const root = createRoot(host);
    const retry = vi.fn();
    const log = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      await act(async () => root.render(<PluginUiHost pluginId="fixture" state={{ status: "loading" }} onRetry={retry} />));
      expect(host.querySelector('[role="status"]')).not.toBeNull();
      await act(async () => root.render(<PluginUiHost pluginId="fixture" state={{ status: "error", reason: "mount failure" }} onRetry={retry} />));
      expect(host.querySelector('[role="alert"]')?.textContent).toContain("mount failure");
      expect(log).toHaveBeenCalledWith(expect.stringContaining("mount failure"));
      await act(async () => [...host.querySelectorAll<HTMLButtonElement>("button")].find(button => button.textContent === "Retry plugin UI")!.click());
      expect(retry).toHaveBeenCalledTimes(1);
      await act(async () => root.render(<PluginUiHost pluginId="fixture" state={{ status: "ready" }} onRetry={retry}><button>Plugin content</button></PluginUiHost>));
      expect(host.querySelector('[role="alert"]')).toBeNull();
      expect(host.querySelector("button")?.textContent).toBe("Plugin content");
      expect(host.querySelector('[aria-hidden="false"]')).not.toBeNull();
    } finally { await act(async () => root.unmount()); log.mockRestore(); }
  });
});
