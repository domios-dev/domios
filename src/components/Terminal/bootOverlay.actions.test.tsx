import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest } from "../../lib/bridge";
import type { Tab } from "../../lib/types";
import { TabBootOverlay, tabBootOverlayView } from "./bootOverlay";
import { useStartNewSession } from "./useStartNewSession";

const win = new Window({ url: "http://localhost" });
Object.defineProperty(globalThis, "window", { value: win, configurable: true });
Object.defineProperty(globalThis, "document", { value: win.document, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: win.HTMLElement, configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const tab: Tab = {
  id: "restored-tab", title: "Volta", pty_id: null, cwd: "/fixture", tab_type: "Terminal",
  agent_type: "fixture-provider", boot_stage: "spawn_failed",
  boot_failure_reason: "restored_session_unavailable: session_missing: opaque-session",
};

async function main() {
  const requests: Array<{ command: string; args?: Record<string, unknown> }> = [];
  let rejectRequest: (error: Error) => void = () => { throw new Error("request not started"); };
  const pending = new Promise<void>((_resolve, reject) => { rejectRequest = reject; });
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>): Promise<T> => {
    requests.push({ command, args });
    await pending;
    return undefined as T;
  });
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  let kept = 0;
  function Card({ currentTab }: { currentTab: Tab }) {
    const action = useStartNewSession(currentTab.id);
    return <TabBootOverlay view={tabBootOverlayView(currentTab)} shellReady overlayVisible
      onRetry={() => { throw new Error("unexpected retry"); }}
      onStartFreshAgent={() => { throw new Error("unexpected picker"); }}
      onKeepShell={() => { kept++; }} {...action} />;
  }
  try {
    await act(async () => { root.render(<Card currentTab={tab} />); });
    const start = host.querySelector<HTMLButtonElement>('button[aria-label="Start new session"]');
    const keep = host.querySelector<HTMLButtonElement>('button[aria-label="Keep using this shell"]');
    assert(start);
    assert(keep);
    await act(async () => { keep.click(); });
    assert.equal(kept, 1);
    assert.equal(requests.length, 0);
    await act(async () => { start.click(); start.click(); });
    assert.equal(start.disabled, true);
    assert.deepEqual(requests, [{ command: "restart_agent_tab", args: { tabId: "restored-tab", fresh: true } }]);
    await act(async () => { rejectRequest(new Error("opaque-refusal")); });
    assert.equal(start.disabled, false);
    assert([...host.querySelectorAll("details pre")].some((element) => element.textContent === "opaque-refusal"));
    await act(async () => {
      root.render(<Card currentTab={{ ...tab, boot_failure_reason: "restored_session_unavailable: opaque-existing-session" }} />);
    });
    assert.equal(host.querySelector('button[aria-label="Start new session"]'), null);
    assert(host.querySelector('button[aria-label="Keep using this shell"]'));
    assert.equal(requests.length, 1);
  } finally {
    await act(async () => { root.unmount(); });
    host.remove();
    __setInvokeForTest(null);
  }
  console.log("✓ unavailable restore card: explicit in-place action, one request, refusal visible");
}

main().catch((error) => { console.error(error); process.exitCode = 1; });
