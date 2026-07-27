import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import {
  recordRecentTab,
  retainLiveTabs,
  scheduleSequentialPrewarm,
  selectPrewarmTabIds,
  TAB_PREWARM_LIMIT,
  useTabPrewarm,
} from "./useTabPrewarm";

test("recent tabs prewarm before adjacent tabs", () => {
  const recent = recordRecentTab(["tab-a", "tab-d"], "tab-c");
  assert.deepEqual(recent, ["tab-c", "tab-a", "tab-d"]);
  assert.deepEqual(selectPrewarmTabIds(["tab-a", "tab-b", "tab-c", "tab-d"], "tab-c", recent), [
    "tab-a",
    "tab-d",
  ]);
});

test("adjacent tabs fill an empty recent set in nearest-first order", () => {
  assert.deepEqual(selectPrewarmTabIds(["tab-a", "tab-b", "tab-c", "tab-d"], "tab-c", []), [
    "tab-b",
    "tab-d",
  ]);
});

test("prewarm stays bounded and ignores tabs outside the active workspace", () => {
  const selected = selectPrewarmTabIds(
    ["tab-a", "tab-b", "tab-c", "tab-d"],
    "tab-b",
    ["foreign", "tab-d", "tab-c", "tab-a"],
  );
  assert.equal(selected.length, TAB_PREWARM_LIMIT);
  assert.deepEqual(selected, ["tab-d", "tab-c"]);
});

test("recent history remains bounded to active plus prewarm capacity", () => {
  assert.deepEqual(recordRecentTab(["tab-b", "tab-c", "tab-d", "tab-e"], "tab-a"), [
    "tab-a",
    "tab-b",
    "tab-c",
  ]);
});

test("prewarm waits for each backend result and cancellation stops the sequence", async () => {
  const frames: Array<() => void> = [];
  const cancelled: number[] = [];
  const mounted: string[] = [];
  let nextHandle = 0;
  const scheduler = {
    request(callback: () => void) {
      frames.push(callback);
      nextHandle += 1;
      return nextHandle;
    },
    cancel(handle: number) {
      cancelled.push(handle);
    },
  };

  const releases: Array<() => void> = [];
  const cancel = scheduleSequentialPrewarm(
    ["tab-a", "tab-b"],
    (id) => new Promise<void>((resolve) => {
      mounted.push(id);
      releases.push(resolve);
    }),
    scheduler,
  );
  assert.deepEqual(mounted, []);
  frames.shift()?.();
  assert.deepEqual(mounted, ["tab-a"]);
  assert.equal(frames.length, 0);
  releases.shift()?.();
  await Promise.resolve();
  cancel();
  frames.shift()?.();
  assert.deepEqual(mounted, ["tab-a"]);
  assert.deepEqual(cancelled, [2]);
});

test("already mounted tabs stay warm until the tab is closed", () => {
  const mounted = new Set(["tab-a", "tab-b", "closed-pane"]);
  assert.deepEqual([...retainLiveTabs(mounted, ["tab-a", "tab-b", "tab-c"])], ["tab-a", "tab-b"]);
});

test("App provider key invokes the hook prewarm callback exactly once", async () => {
  const appSource = readFileSync(new URL("../../App.tsx", import.meta.url), "utf8");
  assert.match(appSource, /prewarmTab:\s*prewarmBackendTab/);
  assert.match(
    appSource,
    /shouldMountTab\s*=\s*isActiveTab\s*\|\|\s*isMainTab\s*\|\|\s*prewarmTabIds\.has\(tab\.id\)/,
  );

  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/prewarm" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });

  const frames: FrameRequestCallback[] = [];
  Object.defineProperty(globalThis, "requestAnimationFrame", {
    configurable: true,
    value: (callback: FrameRequestCallback) => {
      frames.push(callback);
      return frames.length;
    },
  });
  Object.defineProperty(globalThis, "cancelAnimationFrame", {
    configurable: true,
    value: () => {},
  });

  const calls: string[] = [];
  const prewarmTab = async (tabId: string) => {
    calls.push(tabId);
  };
  function Probe() {
    useTabPrewarm({
      workspaceId: "workspace-1",
      activeTabId: "tab-a",
      tabIds: ["tab-a", "tab-b"],
      liveTabIds: ["tab-a", "tab-b"],
      prewarmTab,
    });
    return null;
  }

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(createElement(Probe));
  });
  assert.equal(frames.length, 1);
  await act(async () => {
    const frame = frames.shift();
    assert.ok(frame);
    frame(0);
    await Promise.resolve();
  });
  assert.deepEqual(calls, ["tab-b"]);
  await act(async () => root.unmount());
  host.remove();
});
