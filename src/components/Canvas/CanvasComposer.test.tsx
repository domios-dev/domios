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
    <CanvasComposer api={API} plans={[FIXTURE_PLAN]} layout={layout} viewportCenter={center} mode="workspace" />,
  );

  const chip = host.querySelector('[data-testid="canvas-composer-target"]');
  assert(chip, "target chip rendered");
  assert(chip.textContent?.includes("Orchestrator"), `chip shows owner, got: ${chip.textContent}`);
  assert(chip.textContent?.includes("Auth rollout"), `chip shows plan title, got: ${chip.textContent}`);

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
  assert(text.includes("please review the login endpoint"), "message text sent");
  assert(text.includes("Canvas context"), "auto-attached context present");
  assert(text.includes('plan "Auth rollout"'), "context names the plan");
  assert(text.includes("Dante"), "in-view task attached as context");

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
    <CanvasComposer api={API} plans={[ownerless]} layout={layout} viewportCenter={boxCenter(node.box)} mode="workspace" />,
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
    <CanvasComposer api={API} plans={[FIXTURE_PLAN]} layout={layout} viewportCenter={boxCenter(node.box)} mode="workspace" />,
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
