import { Window } from "happy-dom";
import React, { act } from "react";
import { createRoot } from "react-dom/client";
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import { MessageBubble } from "./MessageBubble";

const windowInstance = new Window({ url: "http://localhost/chat-ui-strides-tests" });
const testWindow = windowInstance as unknown as Window & typeof globalThis;

(globalThis as { window: Window & typeof globalThis }).window = testWindow;
(globalThis as { document: Document }).document = testWindow.document;
Object.defineProperty(globalThis, "navigator", { configurable: true, value: testWindow.navigator });
(globalThis as { HTMLElement: typeof HTMLElement }).HTMLElement = testWindow.HTMLElement;
(globalThis as { HTMLButtonElement: typeof HTMLButtonElement }).HTMLButtonElement = testWindow.HTMLButtonElement;
(globalThis as { Node: typeof Node }).Node = testWindow.Node;
(globalThis as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

const taggedBrief = [
  "opaque-brief-lead-01",
  "",
  '<codeterm-task-strides task="task_ui1" count="3">',
  "Recent task strides (read-only digest, pinned/newest first):",
  "- [decision] 2026-11-25 04:00Z opaque-stride-dec-01",
  "- [worker_report] 2026-11-25 05:00Z opaque-stride-rep-02",
  "- [note] 2026-11-25 06:00Z opaque-stride-note-03",
  "Read full strides: `domios plan task show pl_ui1 task_ui1`",
  "</codeterm-task-strides>",
  "",
  "opaque-brief-tail-02",
].join("\n");

const legacyReminder = [
  "opaque-reminder-lead-10",
  "",
  "Recent task strides (read-only context, pinned/newest first):",
  "- [decision] opaque-legacy-dec-11",
  "- [worker_report] opaque-legacy-rep-12",
  "",
  "opaque-reminder-tail-13",
].join("\n");

async function assertCollapsibleStrides(
  label: string,
  node: React.ReactElement,
  expectedCountText: string,
  expectedHiddenToken: string,
  expectedVisibleToken: string,
  expandOuterCardFirst = false,
) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(node));

  if (expandOuterCardFirst) {
    const outerToggle = host.querySelector<HTMLButtonElement>('[data-testid="chat-system-event"] button[aria-expanded]');
    assert(outerToggle, `${label}: outer system card renders`);
    if (outerToggle.getAttribute("aria-expanded") !== "true") {
      await act(async () => outerToggle.click());
    }
  }

  assert(host.textContent?.includes(expectedVisibleToken), `${label}: surrounding prose stays visible`);
  assert(!host.textContent?.includes("<codeterm-task-strides"), `${label}: raw marker tag never leaks`);

  const strides = host.querySelector<HTMLElement>('[data-testid="chat-task-strides"]');
  assert(strides, `${label}: strides disclosure container renders`);
  const toggle = strides.querySelector<HTMLButtonElement>('[data-testid="chat-task-strides-toggle"]');
  assert(toggle, `${label}: strides toggle button renders`);
  assert(toggle.getAttribute("aria-expanded") === "false", `${label}: strides start collapsed by default`);
  assert(toggle.textContent?.includes(expectedCountText), `${label}: collapsed label shows "${expectedCountText}"`);
  assert(!strides.querySelector('[data-testid="chat-task-strides-body"]'), `${label}: body stays unrendered while collapsed`);
  assert(!host.textContent?.includes(expectedHiddenToken), `${label}: stride body stays hidden while collapsed`);

  await act(async () => toggle.click());
  assert(toggle.getAttribute("aria-expanded") === "true", `${label}: clicking toggle expands strides`);
  const body = strides.querySelector<HTMLElement>('[data-testid="chat-task-strides-body"]');
  assert(body, `${label}: expanded body mounts`);
  assert(body.textContent?.includes(expectedHiddenToken), `${label}: expanded body reveals stride content`);

  await act(async () => toggle.click());
  assert(toggle.getAttribute("aria-expanded") === "false", `${label}: clicking toggle again collapses strides`);
  assert(!strides.querySelector('[data-testid="chat-task-strides-body"]'), `${label}: body unmounts when collapsed again`);

  await act(async () => root.unmount());
  host.remove();
}

async function run() {
  const userBriefMsg: NormalizedChatMessage = {
    id: "user-brief",
    type: "user",
    content: taggedBrief,
  };
  await assertCollapsibleStrides(
    "plain-user-brief",
    <MessageBubble message={userBriefMsg} />,
    "3 task strides",
    "opaque-stride-dec-01",
    "opaque-brief-lead-01",
  );

  const inboundBriefMsg: NormalizedChatMessage = {
    id: "inbound-brief",
    type: "user",
    content: taggedBrief,
    inboundKind: "tab",
    fromLabel: "Elgar",
    fromTab: "4c25e56b",
  };
  await assertCollapsibleStrides(
    "inbound-tab-brief",
    <MessageBubble message={inboundBriefMsg} />,
    "3 task strides",
    "domios plan task show pl_ui1 task_ui1",
    "opaque-brief-tail-02",
  );

  const reminderLegacyMsg: NormalizedChatMessage = {
    id: "reminder-legacy",
    type: "user",
    content: legacyReminder,
  };
  await assertCollapsibleStrides(
    "reminder-card-legacy",
    <MessageBubble message={reminderLegacyMsg} origin={{ kind: "reminder", whoSet: "Producer", id: "rem_1" }} />,
    "2 task strides",
    "opaque-legacy-rep-12",
    "opaque-reminder-tail-13",
  );

  const systemCharterMsg: NormalizedChatMessage = {
    id: "system-charter",
    type: "user",
    content: `<codeterm-worker-charter task="task_ui1">\nopaque-charter-body\n</codeterm-worker-charter>\n\n${taggedBrief}`,
    inboundKind: "system",
    fromContext: "system",
  };
  await assertCollapsibleStrides(
    "system-notice-charter",
    <MessageBubble message={systemCharterMsg} />,
    "3 task strides",
    "opaque-stride-note-03",
    "opaque-charter-body",
    true,
  );

  console.log("ok - task strides render collapsed by default and expand on click across all chat surfaces");
}

void run();
