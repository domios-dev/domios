// Run: npx tsx src/components/CrashReportToast.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest, __setListenForTest } from "../lib/bridge";
import { RecoveryStatusBanner } from "./CrashReportToast";

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
  const window = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "MouseEvent", { value: window.MouseEvent, configurable: true });
}

async function render(ui: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
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

function recoveryState(overrides: Record<string, unknown> = {}) {
  return {
    current: { epoch: "epoch-current", pid: 42, started_at: 1, last_healthy_at: 2 },
    crash_presentation: "active",
    last_crash: {
      classification: "abnormal_exit",
      detected_at: 3,
      new_epoch: "epoch-current",
      previous: { epoch: "epoch-previous", pid: 41, last_healthy_at: 1 },
      supervisor_action: {
        backoff_ms: 1000,
        loop_protected: false,
        crash_count_in_window: 1,
      },
      work_policy: {
        ambiguous_execution: null,
        duplicate_worker_guard: "guarded",
        report_route_guard: "guarded",
      },
    },
    ...overrides,
  };
}

function stubRecovery(state: unknown) {
  __setInvokeForTest(async <T,>(cmd: string): Promise<T> => {
    assert(cmd === "get_recovery_state", "only recovery state should be requested");
    return state as T;
  });
  __setListenForTest(async () => () => undefined);
}

test("crash recovery renders as a dismissible toast, not a full-width banner", async () => {
  setupDom();
  stubRecovery(recoveryState());

  const { host, root } = await render(<RecoveryStatusBanner />);
  const toast = host.querySelector('[data-testid="recovery-status-banner"]');
  assert(toast, "recovery toast should render");
  assert(toast.className.includes("right-3"), "toast should be anchored to the right");
  assert(!toast.className.includes("inset-x-0"), "toast must not span the viewport");

  const close = host.querySelector<HTMLButtonElement>('button[aria-label="Dismiss crash recovery"]');
  assert(close, "toast should expose a dismiss button");
  await act(async () => {
    close.click();
  });

  assert(!host.querySelector('[data-testid="recovery-status-banner"]'), "dismiss should hide this crash");
  await act(async () => root.unmount());
  __setInvokeForTest(null);
  __setListenForTest(null);
});

test("a settled crash record does not resurface on refresh", async () => {
  setupDom();
  stubRecovery(recoveryState({ crash_presentation: "settled" }));

  const { host, root } = await render(<RecoveryStatusBanner />);
  assert(
    !host.querySelector('[data-testid="recovery-status-banner"]'),
    "a healthy current epoch must not carry a stale crash accusation",
  );
  await act(async () => root.unmount());
  __setInvokeForTest(null);
  __setListenForTest(null);
});

test("fully restored bindings raise no ambiguous-work claim", async () => {
  setupDom();
  stubRecovery(recoveryState());

  const { host, root } = await render(<RecoveryStatusBanner />);
  const toast = host.querySelector('[data-testid="recovery-status-banner"]');
  assert(toast, "recovery toast should render");
  assert(
    !toast.textContent?.includes("Ambiguous work"),
    "nothing was lost, so nothing is ambiguous",
  );
  await act(async () => root.unmount());
  __setInvokeForTest(null);
  __setListenForTest(null);
});

test("ambiguous work reports the bindings that never came back", async () => {
  setupDom();
  const state = recoveryState();
  state.last_crash.work_policy.ambiguous_execution = {
    missing_bindings: 2,
    disposition: "needs-review/reassign",
  } as never;
  stubRecovery(state);

  const { host, root } = await render(<RecoveryStatusBanner />);
  const text = host.querySelector('[data-testid="recovery-status-banner"]')?.textContent ?? "";
  assert(text.includes("Ambiguous work"), "a lost binding must surface");
  assert(text.includes("2 bindings not restored"), "the claim must cite the lost bindings");
  assert(text.includes("needs-review/reassign"), "the disposition must remain visible");
  await act(async () => root.unmount());
  __setInvokeForTest(null);
  __setListenForTest(null);
});

async function run() {
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (err) {
      failed++;
      console.error(`  ✗ ${name}`);
      console.error(err);
    }
  }
  if (failed > 0) process.exit(1);
  console.log(`\n✓ ${tests.length}/${tests.length} CrashReportToast tests passed`);
}

void run();
