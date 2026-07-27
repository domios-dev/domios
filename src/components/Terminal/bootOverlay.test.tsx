// Run: npx tsx src/components/Terminal/bootOverlay.test.tsx

import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { TabBootOverlay, tabBootOverlayView } from "./bootOverlay";
import type { Tab } from "../../lib/types";

let failures = 0;
async function test(name: string, fn: () => void | Promise<void>) {
  try {
    await fn();
    console.log(`  ✓ ${name}`);
  } catch (e) {
    failures++;
    console.error(`  ✗ ${name}: ${e instanceof Error ? e.message : String(e)}`);
  }
}

function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

function tab(stage: Tab["boot_stage"], reason?: string | null, canRetry = true): Tab {
  return {
    id: "p-1",
    pty_id: null,
    title: "General",
    protected: canRetry,
    boot_stage: stage,
    boot_failure_reason: reason ?? null,
  } as unknown as Tab;
}

async function render(element: React.ReactElement): Promise<HTMLElement> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(element);
  });
  return host;
}

async function main() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });

  await test("view maps launch stages to Starting", () => {
    assert(tabBootOverlayView(tab("pty_starting")).kind === "starting", "pty_starting starts");
    assert(tabBootOverlayView(tab("agent_launching")).kind === "starting", "agent_launching starts");
  });

  await test("view maps ready to no overlay", () => {
    assert(tabBootOverlayView(tab("agent_ready")).kind === "none", "ready dismisses overlay");
  });

  await test("failure renders details and keyboard-accessible Retry", async () => {
    let clicked = false;
    const host = await render(
      <TabBootOverlay
        view={tabBootOverlayView(tab("spawn_failed", "provider missing"))}
        shellReady={false}
        overlayVisible={false}
        onRetry={() => {
          clicked = true;
        }}
      />,
    );
    assert(host.textContent?.includes("Launch failed"), "failure title visible");
    assert(host.textContent?.includes("provider missing"), "failure details visible");
    const button = host.querySelector("button");
    assert(button !== null, "retry button rendered");
    assert(button.getAttribute("type") === "button", "retry is not submit");
    assert(
      button.getAttribute("aria-label") === "Retry agent launch",
      "retry has accessible name",
    );
    button.click();
    assert(clicked, "retry click propagated");
  });

  await test("non-protected failure has no retry button", async () => {
    const host = await render(
      <TabBootOverlay
        view={tabBootOverlayView(tab("spawn_failed", "no binary", false))}
        shellReady={false}
        overlayVisible={false}
        onRetry={() => {}}
      />,
    );
    assert(host.querySelector("button") === null, "retry hidden for unprotected Tab");
  });

  console.log(`bootOverlay tests: ${failures === 0 ? "all passed" : `${failures} failed`}`);
  if (failures > 0) throw new Error(`${failures} test(s) failed`);
}

void main();
