// Run: npx tsx src/components/Terminal/bootOverlay.wiring.test.tsx

import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { TabBootOverlay, tabBootOverlayView } from "./bootOverlay";
import type { Tab } from "../../lib/types";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const window = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "window", { value: window, configurable: true });
Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

async function main() {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const tab = {
    id: "fixture-tab",
    pty_id: null,
    title: "fixture",
    tab_type: "Terminal",
    protected: false,
    agent_type: null,
    boot_stage: "spawn_failed",
    boot_failure_reason: "opaque-reason-31",
  } as unknown as Tab;
  let starts = 0;
  let keeps = 0;

  await act(async () => {
    root.render(
      <TabBootOverlay
        view={tabBootOverlayView(tab)}
        shellReady={true}
        overlayVisible={false}
        onRetry={() => undefined}
        onStartFreshAgent={() => { starts += 1; }}
        onStartNewSession={() => { throw new Error("unexpected new session"); }}
        onKeepShell={() => { keeps += 1; }}
      />,
    );
  });

  assert(host.querySelector('[role="alert"]'), "failure panel is exposed as an alert");
  const start = host.querySelector('button[aria-label="Start a fresh agent"]');
  const keep = host.querySelector('button[aria-label="Keep using this shell"]');
  assert(start && keep, "both recovery actions have accessible names");
  await act(async () => { (start as HTMLButtonElement).click(); });
  await act(async () => { (keep as HTMLButtonElement).click(); });
  assert(starts === 1, "start-fresh action fires once");
  assert(keeps === 1, "keep-shell action fires once");

  await act(async () => root.unmount());
  host.remove();
  console.log("✓ bootOverlay wiring: alert role, action names, and single handler calls");
}

void main();
