// Run: npx tsx src/components/Terminal/PermissionAutoConfirm.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import {
  AutoConfirmedPromptRows,
  PermissionAutoConfirmToggle,
  autoConfirmationsForSession,
} from "./PermissionAutoConfirm";
import type { AutoConfirmedPrompt } from "../../lib/types";

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
  for (const key of ["window", "document", "navigator"] as const) {
    const value = key === "window" ? window : window[key];
    Object.defineProperty(globalThis, key, { value, configurable: true });
  }
  for (const key of ["HTMLElement", "HTMLButtonElement", "SVGElement", "Element", "Node", "Event", "MouseEvent"] as const) {
    Object.defineProperty(globalThis, key, { value: window[key], configurable: true });
  }
}

async function render(ui: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return { host, root };
}

function record(tool: string, option: string, sessionId: string, fingerprint: string): AutoConfirmedPrompt {
  return {
    tool_name: tool,
    option_label: option,
    rule: `rule-${fingerprint}`,
    question: `question-${fingerprint}`,
    options: [option, "decline"],
    prompt_fingerprint: fingerprint,
    session_id: sessionId,
    confirmed_at_ms: 1,
  };
}

test("only the current session's auto-confirmations are shown", () => {
  const records = [
    record("tool-a", "opt-a", "session-1", "fp-1"),
    record("tool-b", "opt-b", "session-2", "fp-2"),
  ];
  const shown = autoConfirmationsForSession(records, "session-1");
  assert(shown.length === 1 && shown[0].prompt_fingerprint === "fp-1", "filters to the tab's session");
  assert(autoConfirmationsForSession(records, null).length === 0, "an unbound tab shows none");
  assert(autoConfirmationsForSession(undefined, "session-1").length === 0, "absent records show none");
});

test("each auto-confirmation renders one row naming its tool, option and rule", async () => {
  const records = [
    record("tool-a", "opt-a", "session-1", "fp-1"),
    record("tool-b", "opt-b", "session-1", "fp-2"),
  ];
  const { host, root } = await render(<AutoConfirmedPromptRows records={records} />);
  const list = host.querySelector('[data-testid="auto-confirmed-prompts"]');
  assert(list?.getAttribute("aria-label"), "the row list has an accessible name");
  const rows = Array.from(host.querySelectorAll('[data-testid="auto-confirmed-prompt"]'));
  assert(rows.length === 2, `expected 2 rows, got ${rows.length}`);
  rows.forEach((row, index) => {
    assert(row.querySelector('[data-testid="auto-confirmed-tool"]')?.textContent === records[index].tool_name, "row names the tool");
    assert(row.querySelector('[data-testid="auto-confirmed-option"]')?.textContent === records[index].option_label, "row names the option");
    assert(row.getAttribute("data-rule") === records[index].rule, "row carries the matched rule");
    assert(row.getAttribute("title")?.includes(records[index].question), "row keeps the raw prompt");
  });
  await act(async () => root.unmount());
});

test("no auto-confirmations render nothing", async () => {
  const { host, root } = await render(<AutoConfirmedPromptRows records={[]} />);
  assert(host.querySelector('[data-testid="auto-confirmed-prompts"]') === null, "empty list renders nothing");
  await act(async () => root.unmount());
});

test("the toggle is a switch that reports its state and requests the opposite", async () => {
  for (const enabled of [false, true]) {
    const calls: boolean[] = [];
    const { host, root } = await render(
      <PermissionAutoConfirmToggle enabled={enabled} onToggle={(next) => calls.push(next)} />,
    );
    const toggle = host.querySelector<HTMLButtonElement>('[data-testid="permission-auto-confirm-toggle"]');
    assert(toggle, "toggle rendered");
    assert(toggle.getAttribute("role") === "switch", "toggle is a switch");
    assert(toggle.getAttribute("aria-checked") === String(enabled), "switch reports the setting");
    assert(toggle.getAttribute("aria-label"), "switch has an accessible name");
    await act(async () => {
      toggle.click();
    });
    assert(calls.length === 1 && calls[0] === !enabled, `expected one request for ${!enabled}, got ${JSON.stringify(calls)}`);
    await act(async () => root.unmount());
  }
});

async function main() {
  setupDom();
  const started = performance.now();
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`✓ ${name}`);
    } catch (error) {
      failed += 1;
      console.error(`✗ ${name}`);
      console.error(error);
    }
  }
  const elapsedMs = performance.now() - started;
  console.log(`PermissionAutoConfirm tests: ${tests.length - failed}/${tests.length} passed in ${elapsedMs.toFixed(1)}ms`);
  if (failed > 0) throw new Error(`${failed} test(s) failed`);
}

void main();
