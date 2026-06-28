// Run: npx tsx src/components/Canvas/CanvasComposer.test.tsx

import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { FIXTURE_PLAN } from "./cards/fixtures.testdata";
import { CanvasComposer } from "./CanvasComposer";
import { boxCenter, layoutWorkspace } from "./layout";
import type { ApiConfig } from "../../lib/types";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) {
  tests.push([name, fn]);
}
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/canvas-composer" });
  (window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
  for (const key of [
    "window",
    "document",
    "navigator",
    "HTMLElement",
    "HTMLButtonElement",
    "HTMLTextAreaElement",
    "Node",
    "Event",
  ] as const) {
    Object.defineProperty(globalThis, key, {
      value: (window as unknown as Record<string, unknown>)[key],
      configurable: true,
    });
  }
  Object.defineProperty(globalThis, "ResizeObserver", {
    configurable: true,
    value: class {
      observe() {}
      disconnect() {}
    },
  });
  (globalThis as unknown as { requestAnimationFrame: (cb: FrameRequestCallback) => number }).requestAnimationFrame =
    (cb: FrameRequestCallback) => setTimeout(() => cb(Date.now()), 0) as unknown as number;
}

async function render(ui: ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  await act(async () => {
    await Promise.resolve();
  });
  return { host, root };
}

// React tracks the value via a property setter; bypass it with the prototype
// setter so onChange fires, mirroring a real keystroke.
function typeInto(ta: HTMLTextAreaElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(ta), "value")?.set;
  setter?.call(ta, value);
  ta.dispatchEvent(new (globalThis as unknown as { Event: typeof Event }).Event("input", { bubbles: true }));
}

const API: ApiConfig = { host: "127.0.0.1", port: 7686, token: "tok" } as ApiConfig;

// Top-level so react-dom binds to the happy-dom globals before any render.
setupDom();

test("targets the focused plan's owner and POSTs the message + in-view context", async () => {
  const layout = layoutWorkspace([FIXTURE_PLAN]);
  const taskNode = layout.nodeIndex.get("task-dante");
  assert(taskNode, "fixture task node laid out");
  const center = boxCenter(taskNode.box);

  let captured: { url?: string; body?: { text?: string } } = {};
  (globalThis as unknown as { fetch: typeof fetch }).fetch = (async (url: string, init: RequestInit) => {
    captured = { url, body: JSON.parse(String(init.body)) };
    return { ok: true, status: 200, text: async () => "" } as Response;
  }) as unknown as typeof fetch;

  const { host, root } = await render(
    <CanvasComposer api={API} plans={[FIXTURE_PLAN]} layout={layout} viewportCenter={center} />,
  );

  const target = host.querySelector('[data-testid="canvas-composer-target"]');
  assert(target, "target line rendered");
  assert(target.textContent?.includes("Orchestrator"), `target shows owner, got: ${target.textContent}`);

  const chip = host.querySelector('[data-testid="canvas-context-chip"]');
  assert(chip, "context chip rendered");
  assert(chip.getAttribute("data-context-kind") === "task", `chip referent is the centered task, got: ${chip.getAttribute("data-context-kind")}`);
  assert(chip.textContent?.includes("Dante"), `chip names the referent, got: ${chip.textContent}`);

  const ta = host.querySelector('[data-testid="canvas-composer-input"]') as HTMLTextAreaElement;
  assert(ta, "input rendered");
  await act(async () => {
    typeInto(ta, "please review the login endpoint");
  });

  const sendBtn = host.querySelector('[data-testid="canvas-composer-send"]') as HTMLButtonElement;
  assert(sendBtn && !sendBtn.disabled, "send enabled once text + target present");
  await act(async () => {
    sendBtn.click();
  });
  await act(async () => {
    await Promise.resolve();
  });

  assert(captured.url === "http://127.0.0.1:7686/api/panes/orch-1/send", `POST to owner pane, got: ${captured.url}`);
  const text = captured.body?.text ?? "";
  assert(text.startsWith("-=-codeterm:pane"), "rides the pane marker envelope");
  assert(text.includes("please review the login endpoint"), "message text sent");
  assert(text.includes("Canvas context"), "context reference present");
  assert(text.includes("plan plan-alpha"), "context carries the plan id");
  assert(text.includes("task task-dante"), "context carries the centered task id");

  await act(async () => root.unmount());
});

test("disables send and hints when the focused plan has no owner pane", async () => {
  const ownerless = {
    ...FIXTURE_PLAN,
    id: "plan-no-owner",
    owner: { pane_id: null, session_id: null, title: null },
  };
  const layout = layoutWorkspace([ownerless]);
  const node = layout.nodeIndex.get("task-dante");
  assert(node, "node laid out");

  const { host, root } = await render(
    <CanvasComposer api={API} plans={[ownerless]} layout={layout} viewportCenter={boxCenter(node.box)} />,
  );

  const sendBtn = host.querySelector('[data-testid="canvas-composer-send"]') as HTMLButtonElement;
  assert(sendBtn.disabled, "send disabled without an owner pane");

  await act(async () => root.unmount());
});

test("clears the input after a successful send (clear-on-send bug fix)", async () => {
  const layout = layoutWorkspace([FIXTURE_PLAN]);
  const node = layout.nodeIndex.get("task-dante");
  assert(node, "node laid out");
  (globalThis as unknown as { fetch: typeof fetch }).fetch = (async () =>
    ({ ok: true, status: 200, text: async () => "" }) as Response) as unknown as typeof fetch;

  const { host, root } = await render(
    <CanvasComposer api={API} plans={[FIXTURE_PLAN]} layout={layout} viewportCenter={boxCenter(node.box)} />,
  );

  const ta = host.querySelector('[data-testid="canvas-composer-input"]') as HTMLTextAreaElement;
  await act(async () => {
    typeInto(ta, "ship it");
  });
  assert(ta.value === "ship it", "text present before send");

  const sendBtn = host.querySelector('[data-testid="canvas-composer-send"]') as HTMLButtonElement;
  await act(async () => {
    sendBtn.click();
  });
  await act(async () => {
    await Promise.resolve();
  });

  const after = host.querySelector('[data-testid="canvas-composer-input"]') as HTMLTextAreaElement;
  assert(after.value === "", `input cleared after send, got: "${after.value}"`);
  await act(async () => root.unmount());
});

test("surfaces the owner's latest reply (last_turn_summary) in the activity bubble", async () => {
  const layout = layoutWorkspace([FIXTURE_PLAN]);
  const node = layout.nodeIndex.get("task-dante");
  assert(node, "node laid out");
  const panes = {
    "orch-1": { id: "orch-1", agent_state: { state: "idle", last_turn_summary: "Split the login task into 3 subtasks." } },
  } as unknown as Record<string, import("../../lib/types").Pane>;

  const { host, root } = await render(
    <CanvasComposer api={API} plans={[FIXTURE_PLAN]} layout={layout} viewportCenter={boxCenter(node.box)} panes={panes} />,
  );
  const bubble = host.querySelector('[data-testid="canvas-activity-bubble"]');
  assert(bubble, "reply bubble rendered when the owner has a last_turn_summary");
  assert(bubble.getAttribute("data-activity-working") === "false", "shown as a reply, not a working spinner");
  assert(bubble.textContent?.includes("Split the login task"), `bubble shows the reply, got: ${bubble.textContent}`);
  await act(async () => root.unmount());
});

test("open overlay node WINS as the referent (single floating composer, context-switched)", async () => {
  const layout = layoutWorkspace([FIXTURE_PLAN]);
  // Centre points nowhere; the open overlay node must still be the referent.
  const { host, root } = await render(
    <CanvasComposer api={API} plans={[FIXTURE_PLAN]} layout={layout} viewportCenter={{ x: -99999, y: -99999 }}
      overlay={{ id: "task-dante", kind: "task", planId: "plan-alpha" }} />,
  );
  const chip = host.querySelector('[data-testid="canvas-context-chip"]');
  assert(chip?.getAttribute("data-context-id") === "task-dante", "referent switched to the open overlay node");
  const target = host.querySelector('[data-testid="canvas-composer-target"]');
  assert(target?.getAttribute("data-owner-pane") === "orch-1", "routes to that plan's owner orchestrator");
  await act(async () => root.unmount());
});

async function main() {
  let passed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      passed++;
      console.log(`  ✓ ${name}`);
    } catch (e) {
      console.error(`  ✗ ${name}`);
      console.error(`    ${e instanceof Error ? e.stack : e}`);
      process.exit(1);
    }
  }
  console.log(`\n✓ ${passed}/${tests.length} CanvasComposer tests passed`);
}

main();
