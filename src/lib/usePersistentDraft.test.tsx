import { useEffect } from "react";
import { createRoot, type Root } from "react-dom/client";
import { act } from "react";
import { Window } from "happy-dom";
import { frontendSignals } from "./frontendUpdate/appState";
import { usePersistentDraft } from "./usePersistentDraft";

type TestFn = () => void | Promise<void>;
const failures: string[] = [];
const tests: Array<{ name: string; fn: TestFn }> = [];

function test(name: string, fn: TestFn) {
  tests.push({ name, fn });
}

function assert(condition: unknown, message: string) {
  if (!condition) throw new Error(message);
}

interface HarnessHandle {
  setValue: (value: string) => void;
  value: string;
}

function setupDom() {
  frontendSignals.hydrate({ activeWorkspaceId: null, activeTabId: null, drafts: {}, scroll: {} });
  const win = new Window({ url: "http://localhost" });
  Object.assign(globalThis, {
    window: win,
    document: win.document,
    localStorage: win.localStorage,
    Event: win.Event,
    IS_REACT_ACT_ENVIRONMENT: true,
  });
  const host = document.createElement("div");
  document.body.appendChild(host);
  let root: Root | null = createRoot(host);
  const handle: HarnessHandle = {
    setValue: () => {},
    value: "",
  };
  const render = async (scope: string) => {
    await act(async () => {
      root?.render(<Harness scope={scope} handle={handle} />);
    });
  };
  const unmount = async () => {
    await act(async () => root?.unmount());
    root = null;
  };
  return { handle, render, unmount, storage: win.localStorage, win };
}

function Harness({ scope, handle }: { scope: string; handle: HarnessHandle }) {
  const [value, setValue] = usePersistentDraft(scope);
  handle.value = value;
  handle.setValue = setValue;
  useEffect(() => {
    handle.value = value;
  }, [handle, value]);
  return <textarea value={value} onChange={(e) => setValue(e.target.value)} />;
}

test("captures rapid edits and stores the exact final value synchronously", async () => {
  const { handle, render, storage, unmount } = setupDom();
  await render("rapid");
  await act(async () => {
    handle.setValue("a");
    handle.setValue("ab");
    handle.setValue("abc");
  });
  assert(frontendSignals.checkpoint().drafts.rapid.text === "abc", "collector missed the final edit");
  assert(storage.getItem("codeterm:composer-draft:rapid") === "abc", "final value not stored");
  await unmount();
});

test("flushes old scope before switching and never writes into the new scope", async () => {
  const { handle, render, storage, unmount } = setupDom();
  await render("scope-a");
  await act(async () => handle.setValue("alpha"));
  await render("scope-b");
  assert(storage.getItem("codeterm:composer-draft:scope-a") === "alpha", "old scope was not flushed");
  assert(storage.getItem("codeterm:composer-draft:scope-b") === null, "old value leaked into new scope");
  await act(async () => handle.setValue("beta"));
  await unmount();
  assert(storage.getItem("codeterm:composer-draft:scope-b") === "beta", "new scope did not flush on unmount");
});

test("flushes on blur and pagehide", async () => {
  const { handle, render, storage, win, unmount } = setupDom();
  await render("events");
  await act(async () => handle.setValue("blurred"));
  win.dispatchEvent(new win.Event("blur"));
  assert(storage.getItem("codeterm:composer-draft:events") === "blurred", "blur did not flush");
  await act(async () => handle.setValue("hidden"));
  win.dispatchEvent(new win.Event("pagehide"));
  assert(storage.getItem("codeterm:composer-draft:events") === "hidden", "pagehide did not flush");
  await unmount();
});

test("quota failures are swallowed and controlled value remains live", async () => {
  const { handle, render, storage, unmount } = setupDom();
  const original = storage.setItem.bind(storage);
  storage.setItem = () => {
    throw new Error("quota");
  };
  await render("quota");
  await act(async () => handle.setValue("survives"));
  assert(handle.value === "survives", "controlled value was lost after quota failure");
  storage.setItem = original;
  await unmount();
});

test("registers local fallback with the collector and restores daemon data without local storage", async () => {
  const { handle, render, storage, unmount } = setupDom();
  storage.setItem("codeterm:composer-draft:tab-session-chat:restored", "unsent fallback");
  await render("tab-session-chat:restored");
  assert(handle.value === "unsent fallback", "fallback was not restored");
  assert(frontendSignals.checkpoint().drafts["tab-session-chat:restored"].text === "unsent fallback", "fallback missing from collector");
  storage.clear();
  frontendSignals.hydrate({ activeWorkspaceId: null, activeTabId: null, drafts: {
    "tab-session-chat:daemon": { text: "daemon draft", attachmentIds: ["image1"], replyTo: null, capsules: { image1: { label: "image1", path: "/uploads/image.png", kind: "image" } } },
  }, scroll: {} });
  await render("tab-session-chat:daemon");
  assert(handle.value === "daemon draft", "checkpoint draft was not restored");
  assert(storage.length === 0, "restore depended on local storage");
  assert(frontendSignals.checkpoint().drafts["tab-session-chat:daemon"].attachmentIds[0] === "image1", "metadata was lost on mount");
  await render("tab-session-chat-capsules:daemon");
  assert(handle.value.includes("/uploads/image.png"), "attachment reference was not restored");
  await unmount();
});

(async () => {
  for (const { name, fn } of tests) {
    try {
      await fn();
      console.log(`ok - ${name}`);
    } catch (e) {
      failures.push(name);
      console.error(`FAIL - ${name}:`, e);
    }
  }
  if (failures.length > 0) process.exit(1);
  console.log("usePersistentDraft checks passed");
})();
