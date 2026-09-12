// Run: npx tsx src/components/FrontendCrashBoundary.test.tsx

import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { MultiplexerState } from "../lib/types";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const CRASHING_URL = "http://blackstar.example/app?e2e=1#relay=peer-1";
const PEER_ID = "peer-1";
const CRASH_MESSAGE = "render exploded with an actionable message";
const LONG_MINIFIED_FRAME = `    at t (https://blackstar.example/assets/index-9f8e7d6c.js?source=${"x".repeat(220)}:1:48291)`;
const CRASH_STACK = [`Error: ${CRASH_MESSAGE}`, LONG_MINIFIED_FRAME, "    at n (https://blackstar.example/assets/index-9f8e7d6c.js:1:48302)"].join("\n");

const testWindow = new Window({ url: CRASHING_URL });
for (const key of [
  "window",
  "document",
  "localStorage",
  "sessionStorage",
  "HTMLElement",
  "HTMLButtonElement",
  "Element",
  "Node",
  "DOMException",
] as const) {
  Object.defineProperty(globalThis, key, {
    value: (testWindow as unknown as Record<string, unknown>)[key],
    configurable: true,
  });
}
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let brokenRenders = 0;
const renderError = new Error(CRASH_MESSAGE);
renderError.stack = CRASH_STACK;

function BrokenView(): React.ReactNode {
  brokenRenders += 1;
  throw renderError;
}

function HealthyView(): React.ReactNode {
  return <p>workspace rendered</p>;
}

async function mount(node: React.ReactNode): Promise<HTMLElement> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const originalError = console.error;
  console.error = () => {};
  await act(async () => {
    root.render(node);
  });
  console.error = originalError;
  return host as unknown as HTMLElement;
}

const CRASHING_SNAPSHOT: MultiplexerState = {
  workspaces: [],
  tabs: {},
  active_workspace_id: "ws-no-teams",
  focused_tab_id: null,
};

async function main() {
  const { FrontendCrashBoundary, SAFE_START_KEY } = await import("./FrontendCrashBoundary");
  const { clearRelayTarget, relayTarget, setRelayTarget } = await import("../lib/webApi");
  const { loadCachedStateSync, saveStateSnapshot } = await import("../lib/stateCache");

  setRelayTarget(PEER_ID, "Blackstar");
  saveStateSnapshot(CRASHING_SNAPSHOT);
  assert(
    loadCachedStateSync()?.active_workspace_id === "ws-no-teams",
    "the crashing workspace really is the location a reload would restore",
  );

  const navigated: string[] = [];
  const crashHost = await mount(
    <FrontendCrashBoundary navigate={(url) => navigated.push(url)}>
      <BrokenView />
    </FrontendCrashBoundary>,
  );

  assert(crashHost.querySelector('[role="alert"]'), "render errors show an in-app crash surface");
  const messageDetail = crashHost.querySelector<HTMLElement>("#frontend-crash-message");
  const stackDetail = crashHost.querySelector<HTMLElement>("#frontend-crash-stack");
  assert(messageDetail, "crash surface has a stable error-message detail");
  assert(stackDetail, "crash surface has a stable stack-trace detail");
  assert(messageDetail.dataset.testid === "frontend-crash-message", "error message has a test anchor");
  assert(stackDetail.dataset.testid === "frontend-crash-stack", "stack trace has a test anchor");
  assert(messageDetail.classList.contains("frontend-crash__message"), "error message has a readable presentation anchor");
  assert(stackDetail.classList.contains("frontend-crash__stack"), "stack trace has a contained presentation anchor");
  assert(messageDetail.textContent?.trim() === CRASH_MESSAGE, "crash surface presents the thrown message as primary detail");
  assert(stackDetail.textContent?.includes(LONG_MINIFIED_FRAME), "long minified frames remain available in the stack detail");
  assert(!stackDetail.textContent?.includes(CRASH_MESSAGE), "the stack detail does not duplicate its message header");
  assert(
    Boolean(messageDetail.compareDocumentPosition(stackDetail) & Node.DOCUMENT_POSITION_FOLLOWING),
    "the error message appears before the stack detail",
  );
  assert(crashHost.querySelector("[data-tauri-drag-region]"), "crash surface keeps a draggable titlebar");
  assert(crashHost.textContent?.includes("Reload interface"), "crash surface keeps the reload action");

  const escapeButton = Array.from(crashHost.querySelectorAll("button")).find((button) =>
    button.textContent?.includes("Leave this location"),
  );
  assert(escapeButton, "crash surface offers an escape that is not a reload");

  await act(async () => escapeButton.click());

  assert(navigated.length === 1, "the escape navigates exactly once");
  // Before the exact-match assertion below, which narrows the value to a literal
  // and would make this comparison vacuous to the compiler.
  assert(navigated[0] !== CRASHING_URL, "the escape destination is not the location that crashed");
  assert(
    navigated[0] === "http://blackstar.example/app?e2e=1#relay=self",
    "the escape targets a safe-start URL on this machine",
  );
  assert(navigated[0].endsWith("#relay=self"), "the escape leaves the relayed machine for this one");
  assert(relayTarget() === null, "the escape drops the stored remote machine target");

  setRelayTarget(PEER_ID, "Blackstar");
  assert(
    loadCachedStateSync() === null,
    "the peer-scoped snapshot that painted the crashing workspace is gone",
  );
  clearRelayTarget();

  assert(
    window.sessionStorage.getItem(SAFE_START_KEY) === "1",
    "the escape arms a safe start for the load it triggers",
  );

  brokenRenders = 0;
  const safeHost = await mount(
    <FrontendCrashBoundary>
      <BrokenView />
    </FrontendCrashBoundary>,
  );

  assert(brokenRenders === 0, "the load after the escape never re-renders the crashing tree");
  assert(safeHost.textContent?.includes("Left the location that crashed"), "the escape lands on a safe start root");
  assert(safeHost.querySelector('[role="status"]'), "the safe start root announces itself");
  assert(safeHost.querySelector("[data-tauri-drag-region]"), "the safe start root keeps a draggable titlebar");
  assert(!safeHost.querySelector('[role="alert"]'), "the safe start root is not the crash alert again");
  assert(
    window.sessionStorage.getItem(SAFE_START_KEY) === null,
    "the safe start is consumed once so later loads are normal",
  );

  const normalHost = await mount(
    <FrontendCrashBoundary>
      <HealthyView />
    </FrontendCrashBoundary>,
  );
  assert(normalHost.textContent?.includes("workspace rendered"), "a later load mounts the app normally");

  console.log("✓ a render crash offers a non-looping escape to a safe start");
}

void main();
