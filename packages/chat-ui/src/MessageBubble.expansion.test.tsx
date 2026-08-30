import { Window } from "happy-dom";
import React, { act } from "react";
import { createRoot } from "react-dom/client";

import { reduceToolEvents, type NormalizedChatMessage } from "@codeterm/chat-core";
import { MessageBubble } from "./MessageBubble";

const windowInstance = new Window({ url: "http://localhost/chat-ui-expansion-tests" });
const testWindow = windowInstance as unknown as Window & typeof globalThis;

(globalThis as { window: Window & typeof globalThis }).window = testWindow;
(globalThis as { document: Document }).document = testWindow.document;
Object.defineProperty(globalThis, "navigator", {
  configurable: true,
  value: testWindow.navigator,
});
(globalThis as { HTMLElement: typeof HTMLElement }).HTMLElement = testWindow.HTMLElement;
(globalThis as { HTMLButtonElement: typeof HTMLButtonElement }).HTMLButtonElement = testWindow.HTMLButtonElement;
(globalThis as { Node: typeof Node }).Node = testWindow.Node;
(globalThis as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

function command(id: string, detail: string): NormalizedChatMessage {
  return {
    id,
    type: "tool_call",
    content: "",
    toolId: `${id}-tool`,
    toolKind: "command",
    toolSummary: detail,
    toolDetail: detail,
  };
}

async function run() {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const messages = [
    command("expanded-send", "codeterm send --tab worker-1 \"Please review the change\""),
    command("collapsed-tab-list", "codeterm tab list --workspace codeterm"),
  ];
  const completedRead = command("collapsed-read", "src/coordination.ts");
  const completedCommand = command("collapsed-command", "git status --short");
  const completedEdit = command("expanded-edit", "edit src/coordination.ts");
  const systemEvent: NormalizedChatMessage = {
    id: "collapsed-system-event",
    type: "user",
    content: "Internal lifecycle detail",
    inboundKind: "system",
  };
  const ordinaryUser: NormalizedChatMessage = {
    id: "ordinary-user-view",
    type: "user",
    content: "## User formatted\n\n- item",
  };
  const ordinaryAssistant: NormalizedChatMessage = {
    id: "ordinary-assistant-view",
    type: "assistant",
    content: "## Assistant formatted\n\n- item",
  };

  await act(async () => {
    root.render(
      <div>
        {reduceToolEvents(messages).map((item) => {
          const message = item.kind === "message" ? item.message : item.event.message;
          return <MessageBubble key={message.id} message={message} item={item.kind === "tool" ? item : undefined} />;
        })}
        <MessageBubble
          message={completedRead}
          item={{ kind: "tool", event: { id: completedRead.id, phase: "result", status: "success", kind: "read", summary: "src/coordination.ts", result: "private inspection output", message: completedRead } }}
        />
        <MessageBubble
          message={completedCommand}
          item={{ kind: "tool", event: { id: completedCommand.id, phase: "result", status: "success", kind: "command", summary: "git status --short", detail: "git status --short", result: "clean", message: completedCommand } }}
        />
        <MessageBubble
          message={completedEdit}
          item={{ kind: "tool", event: { id: completedEdit.id, phase: "result", status: "success", kind: "edit", summary: "src/coordination.ts", edits: [{ path: "src/coordination.ts", old: "old", new: "new" }], message: completedEdit } }}
        />
        <MessageBubble message={systemEvent} origin={{ kind: "system", context: "lifecycle" }} />
        <MessageBubble message={ordinaryUser} />
        <MessageBubble message={ordinaryAssistant} />
      </div>,
    );
  });

  const cards = Array.from(host.querySelectorAll<HTMLElement>('[data-testid="chat-tool-call"]'));
  const send = cards.find((card) => card.textContent?.includes("Sent to"));
  const list = cards.find((card) => card.textContent?.includes("tab list"));
  const read = cards.find((card) => card.textContent?.includes("Read") && card.textContent.includes("coordination.ts"));
  const completedCommandCard = cards.find((card) => card.textContent?.includes("git status --short"));
  const edit = cards.find((card) => card.querySelector('[data-testid="toolcard-edit-summary"]'));
  assert(send?.querySelector('[aria-label="Hide full command"]'), "outbound message should start expanded");
  assert(list?.querySelector('[aria-label="Show full command"]'), "ordinary operational command should start collapsed");
  assert(read?.querySelector('[aria-label="Expand tool details"]'), "completed read-only inspection should start collapsed");
  assert(!read?.textContent?.includes("private inspection output"), "collapsed read-only inspection should hide output");
  assert(completedCommandCard?.querySelector('[aria-label="Show full command"]'), "completed shell command should start collapsed");
  assert(edit?.querySelector('[aria-label="Collapse tool details"]'), "completed code edit should stay visibly expanded");
  const system = host.querySelector<HTMLElement>('[data-testid="chat-system-event"]');
  const systemToggle = system?.querySelector<HTMLButtonElement>("button");
  assert(systemToggle?.getAttribute("aria-expanded") === "false", "system event should start collapsed");
  assert(!system?.textContent?.includes("Internal lifecycle detail"), "collapsed system event should hide its body");
  await act(async () => systemToggle?.click());
  assert(system?.textContent?.includes("Internal lifecycle detail"), "system event should expand on demand");
  const editDetailsId = edit?.querySelector("button")?.getAttribute("aria-controls");
  const editDetails = editDetailsId ? edit?.querySelector<HTMLElement>(`#${editDetailsId}`) : null;
  assert(editDetails?.style.maxHeight === "600px" && editDetails.style.overflowY === "auto", "expanded code output should use the 600px internal scroll cap");
  const messageViews = Array.from(host.querySelectorAll<HTMLElement>('[data-testid="message-view-toggle"]'));
  assert(messageViews.length === 2, "view toggles render only inside expanded tool cards");
  assert(messageViews.every((view) => view.closest('[data-testid="chat-tool-call"]')), "view toggles live inside tool cards");
  assert(messageViews.every((view) => !view.textContent?.includes("User formatted") && !view.textContent?.includes("Assistant formatted")), "ordinary messages have no view toggle");
  const commandDisclosure = completedCommandCard?.querySelector<HTMLButtonElement>('[aria-label="Show full command"]');
  await act(async () => commandDisclosure?.click());
  const commandView = completedCommandCard?.querySelector<HTMLElement>('[data-testid="message-view-toggle"]');
  assert(commandView && !commandView.querySelector('[data-testid="message-raw-content"]'), "formatted tool view hides the full invocation");
  assert(commandView.textContent?.includes("clean"), "formatted tool view keeps the result visible");
  const commandRaw = Array.from(commandView?.querySelectorAll("button") ?? []).find((button) => button.textContent === "Raw");
  await act(async () => commandRaw?.click());
  assert(commandRaw?.getAttribute("aria-pressed") === "true", "tool card switches to raw view");
  assert(commandView?.querySelector('[data-testid="message-raw-content"]')?.textContent === "git status --short", "raw tool view preserves the full invocation");
  assert(commandView?.textContent?.includes("clean"), "raw tool view keeps the result visible");
  const commandFormatted = Array.from(commandView?.querySelectorAll("button") ?? []).find((button) => button.textContent === "Formatted");
  await act(async () => commandFormatted?.click());
  assert(!commandView?.querySelector('[data-testid="message-raw-content"]') && commandView?.textContent?.includes("clean"), "formatted mode hides the invocation without hiding the result");

  await act(async () => root.unmount());
  host.remove();
  console.log("ok - chat expansion defaults");
}

void run();
