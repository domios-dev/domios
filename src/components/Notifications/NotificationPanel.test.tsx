import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { Notification } from "../../lib/types";
import { __setInvokeForTest } from "../../lib/bridge";
import { NotificationPanel } from "./NotificationPanel";

let checked = 0;

function assert(condition: unknown, message: string): asserts condition {
  checked++;
  if (!condition) throw new Error(message);
}

const window = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "window", { value: window, configurable: true });
Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
Object.defineProperty(globalThis, "PointerEvent", { value: window.PointerEvent, configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const asked: Notification = {
  id: "notice-1",
  tab_id: "tab-tab-1",
  workspace_id: "workspace-1",
  title: "Which branch?",
  body: "Rebase keeps history linear.",
  severity: "Warning",
  timestamp: Date.now(),
  read: false,
  actions: [
    { label: "Rebase", type: "answer" },
    { label: "Merge", type: "answer" },
  ],
  answer: { asked_from_tab: "tab-tab-1", answered: null, delivered: false },
};

const host = document.createElement("div");
document.body.appendChild(host);
const root = createRoot(host);

const answered: Array<[string, string]> = [];
const invoked: Array<{ command: string; args: unknown }> = [];

async function render(notification: Notification) {
  await act(async () => {
    root.render(
      <NotificationPanel
        notifications={[notification]}
        onMarkRead={() => {}}
        onMarkAllRead={() => {}}
        onDelete={() => {}}
        onClearAll={() => {}}
        onJumpToTab={() => {}}
        onAnswer={(id, label) => answered.push([id, label])}
        onClose={() => {}}
      />,
    );
  });
  return host.querySelector('[data-testid="notification-panel"]') as HTMLElement;
}

function buttonLabels(panel: HTMLElement): string[] {
  return Array.from(panel.querySelectorAll("button"))
    .map((button) => button.textContent ?? "")
    .filter((label) => label === "Rebase" || label === "Merge");
}

async function run() {
  __setInvokeForTest(async (command, args) => {
    invoked.push({ command, args });
    return undefined as never;
  });

  // An unanswered question renders one button per offered choice, and tapping
  // one delivers the label alone — no command name, no arguments.
  let panel = await render(asked);
  assert(buttonLabels(panel).join() === "Rebase,Merge", "each offered choice should render a button");
  const rebase = Array.from(panel.querySelectorAll("button")).find(
    (button) => button.textContent === "Rebase",
  ) as HTMLElement;
  await act(async () => rebase.click());
  assert(
    answered.length === 1 && answered[0][0] === asked.id && answered[0][1] === "Rebase",
    "tapping a choice should deliver the notification id and the chosen label",
  );
  const invokedAfterChoice = invoked.length;
  assert(
    invokedAfterChoice === 0,
    "a choice must not reach the generic action runner that can invoke commands",
  );

  // Once answered the row is a record of the decision, not a live control.
  panel = await render({
    ...asked,
    answer: { asked_from_tab: "tab-tab-1", answered: "Rebase", delivered: true },
  });
  const mark = panel.querySelector('[data-testid="notification-answered"]');
  assert(mark, "an answered question should render the chosen answer");
  assert(mark.textContent?.includes("Rebase"), "the rendered answer should name the chosen label");
  assert(
    buttonLabels(panel).length === 0,
    "an answered question should render no choice buttons at all",
  );

  const before = answered.length;
  await act(async () => {
    (panel.querySelector('[data-testid="notification-answered"]') as HTMLElement).click();
  });
  assert(answered.length === before, "the recorded answer is not a control");

  // The recorded tab is gone: the card says so instead of offering buttons that
  // would quietly do nothing.
  panel = await render({
    ...asked,
    answer: { asked_from_tab: null, answered: null, delivered: false },
  });
  assert(
    panel.querySelector('[data-testid="notification-undeliverable"]'),
    "a question whose tab is gone should render as undeliverable",
  );
  assert(
    buttonLabels(panel).length === 0,
    "an undeliverable question should offer no choice buttons",
  );

  // An answer the envelope path could not carry is not a decision the asking
  // tab has: the card says so instead of showing it as answered-and-delivered.
  panel = await render({
    ...asked,
    answer: { asked_from_tab: "tab-tab-1", answered: "Rebase", delivered: false },
  });
  const undelivered = panel.querySelector('[data-testid="notification-answer-undelivered"]');
  assert(undelivered, "an undelivered answer should render as not delivered");
  assert(
    undelivered.textContent?.includes("Rebase"),
    "the undelivered card should still name the standing answer",
  );
  assert(
    !panel.querySelector('[data-testid="notification-answered"]'),
    "an undelivered answer must not render as answered-and-delivered",
  );
  assert(
    buttonLabels(panel).length === 0,
    "a standing answer offers no choice buttons, delivered or not",
  );

  // The pre-existing action kinds still render and still run through the
  // generic runner, untouched by the new kind.
  panel = await render({
    ...asked,
    actions: [
      {
        label: "Update",
        type: "invoke",
        value: "respond_provider_update_prompt",
        args: { promptId: "prompt-1", accept: true },
      },
    ],
    answer: undefined,
  });
  const update = Array.from(panel.querySelectorAll("button")).find(
    (button) => button.textContent === "Update",
  ) as HTMLElement;
  assert(update, "an invoke action should still render its button");
  await act(async () => update.click());
  assert(
    invoked.length === 1 && invoked[0].command === "respond_provider_update_prompt",
    "the pre-existing invoke path should be unchanged",
  );

  // The expand control the panel already had is reused, not replaced.
  const expand = panel.querySelector<HTMLButtonElement>(
    '[aria-label="Show complete message for Which branch?"]',
  );
  assert(expand, "the existing expand control should survive");
  await act(async () => expand.click());
  assert(
    panel.textContent?.includes("Rebase keeps history linear."),
    "expanding should still reveal the complete message",
  );

  await act(async () => root.unmount());
  __setInvokeForTest(null);
  console.log(`NotificationPanel tests: ${checked} assertions passed`);
}

void run();
