import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { setTerminalReadingHistory } from "./terminalHistoryReaders";
import {
  MOUNTED_TAB_CAPACITY,
  recordRecentTab,
  retainMountedTabs,
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

const never = () => false;

function installDom(): FrameRequestCallback[] {
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
  return frames;
}

test("mounted tabs keep the most recently active ones and drop the least recent", () => {
  const live = ["tab-a", "tab-b", "tab-c", "tab-d", "tab-e"];
  let mounted: readonly string[] = [];
  for (const id of live) mounted = retainMountedTabs(mounted, id, live, never, 3);
  assert.deepEqual(mounted, ["tab-e", "tab-d", "tab-c"]);
  mounted = retainMountedTabs(mounted, "tab-c", live, never, 3);
  assert.deepEqual(mounted, ["tab-c", "tab-e", "tab-d"]);
});

test("closed tabs drop from the mounted set", () => {
  assert.deepEqual(retainMountedTabs(["tab-a", "closed-tab", "tab-b"], null, ["tab-a", "tab-b"], never), ["tab-a", "tab-b"]);
});

test("a tab scrolled into terminal history stays mounted past the capacity", () => {
  const live = ["tab-a", "tab-b", "tab-c", "tab-d"];
  const reading = (id: string) => id === "tab-a";
  assert.deepEqual(retainMountedTabs(["tab-c", "tab-b", "tab-a"], "tab-d", live, reading, 2), ["tab-d", "tab-c", "tab-a"]);
});

test("an unchanged mounted set keeps its identity", () => {
  const mounted = ["tab-a", "tab-b"];
  assert.equal(retainMountedTabs(mounted, "tab-a", ["tab-a", "tab-b"], never), mounted);
  assert.equal(retainMountedTabs(mounted, null, ["tab-a", "tab-b"], never), mounted);
});

test("App provider key invokes the hook prewarm callback exactly once", async () => {
  const appSource = readFileSync(new URL("../../App.tsx", import.meta.url), "utf8");
  assert.match(appSource, /prewarmTab:\s*prewarmBackendTab/);
  assert.match(
    appSource,
    /shouldMountTab\s*=\s*!ws\.cwd_missing\s*&&\s*\(isActiveTab\s*\|\|\s*isMainTab\s*\|\|\s*prewarmTabIds\.has\(tabId\)\)/,
  );

  const frames = installDom();

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

test("switching through many tabs keeps the mounted set bounded and keeps a history reader", async () => {
  installDom();
  const tabsPerWorkspace = MOUNTED_TAB_CAPACITY + 2;
  const workspaces = {
    "ws-1": Array.from({ length: tabsPerWorkspace }, (_, index) => `a${index + 1}`),
    "ws-2": Array.from({ length: tabsPerWorkspace }, (_, index) => `b${index + 1}`),
  };
  const live = [...workspaces["ws-1"], ...workspaces["ws-2"]];
  let mounted: ReadonlySet<string> = new Set();
  function Probe({ workspaceId, activeTabId }: { workspaceId: "ws-1" | "ws-2"; activeTabId: string }) {
    mounted = useTabPrewarm({
      workspaceId,
      activeTabId,
      tabIds: workspaces[workspaceId],
      liveTabIds: live,
      prewarmTab: async () => {},
    });
    return null;
  }
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const visit = (workspaceId: "ws-1" | "ws-2", activeTabId: string) =>
    act(async () => root.render(createElement(Probe, { workspaceId, activeTabId })));

  await visit("ws-1", "a1");
  setTerminalReadingHistory("a1", true);
  for (const id of workspaces["ws-1"].slice(1)) await visit("ws-1", id);
  for (const id of workspaces["ws-2"]) await visit("ws-2", id);
  assert.deepEqual([...mounted], [...workspaces["ws-2"]].reverse().slice(0, MOUNTED_TAB_CAPACITY).concat("a1"));

  setTerminalReadingHistory("a1", false);
  await visit("ws-1", "a2");
  assert.equal(mounted.size, MOUNTED_TAB_CAPACITY);
  assert.ok(mounted.has("a2"));
  assert.ok(!mounted.has("a1"));

  await act(async () => root.unmount());
  host.remove();
});
