import { Window } from "happy-dom";
import { describe, expect, it, vi } from "vitest";
import { PLUGIN_UI_MONITOR } from "./pluginUiMonitor";

async function fixture(run: (win: Window, send: ReturnType<typeof vi.fn>) => Promise<void>) {
  const win = new Window();
  const send = vi.fn();
  Object.defineProperty(win, "parent", { value: { postMessage: send } });
  win.document.body.innerHTML = '<div id="ct-root"></div>';
  win.eval(PLUGIN_UI_MONITOR);
  win.document.dispatchEvent(new win.Event("DOMContentLoaded"));
  try { await run(win, send); } finally { await win.close(); }
}

describe("plugin document runtime reporting", () => {
  it("reports an empty builder mount instead of a silent blank frame", async () => {
    await fixture(async (_win, send) => {
      expect(send).toHaveBeenCalledWith({ __ctUiState: { status: "empty", reason: expect.stringContaining("#ct-root") } }, "*");
    });
  });
  it("reports a mount exception even if the bridge already ran", async () => {
    await fixture(async (win, send) => {
      win.dispatchEvent(new win.ErrorEvent("error", { message: "Target container is not a DOM element." }));
      expect(send).toHaveBeenLastCalledWith({ __ctUiState: { status: "error", reason: "Target container is not a DOM element." } }, "*");
      win.document.getElementById("ct-root")!.innerHTML = "<button>content</button>";
      await Promise.resolve();
      expect(send).toHaveBeenLastCalledWith({ __ctUiState: { status: "error", reason: "Target container is not a DOM element." } }, "*");
    });
  });
  it("observes rendered content without a timer", async () => {
    await fixture(async (win, send) => {
      win.document.getElementById("ct-root")!.innerHTML = "<button>content</button>";
      await new Promise<void>((resolve) => {
        const observer = new win.MutationObserver(() => { observer.disconnect(); resolve(); });
        observer.observe(win.document.body, { childList: true, subtree: true });
        win.document.getElementById("ct-root")!.append("ready");
      });
      expect(send).toHaveBeenLastCalledWith({ __ctUiState: { status: "ready", reason: undefined } }, "*");
    });
  });
});
