import { Window } from "happy-dom";
import React, { act } from "react";
import { createRoot } from "react-dom/client";

import { reduceToolEvents, type NormalizedChatMessage } from "@codeterm/chat-core";
import { detectChatPrefix } from "../../../src/lib/chatPrefixes";
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
  const tabPrefix = detectChatPrefix('<domios from="tab" label="Auditor" tab="tab-dom-12" model="model-dom-4">/agent-pad/pointer/opaque-dom-tab-12.md</domios>');
  assert(tabPrefix?.origin.kind === "tab", "parser supplies the Tab origin used by the DOM fixture");
  const reportPrefix = detectChatPrefix('<domios from="report" label="Worker" tab="tab-report-dom-13" model="model-report-dom-5">/agent-pad/pointer/opaque-dom-report-13.md</domios>');
  assert(reportPrefix?.origin.kind === "tab" && reportPrefix.origin.report, "parser supplies the report origin used by the DOM fixture");
  const systemPrefix = detectChatPrefix('<domios from="system" context="sYsTeM">/agent-pad/pointer/opaque-dom-system-14.md</domios>');
  assert(systemPrefix?.origin.kind === "system", "parser supplies the System origin used by the DOM fixture");
  const tabPointerMessage: NormalizedChatMessage = {
    id: "parsed-tab-pointer-message",
    type: "user",
    content: tabPrefix.body,
    inboundKind: "tab",
  };
  const reportPointerMessage: NormalizedChatMessage = {
    id: "parsed-report-pointer-message",
    type: "user",
    content: reportPrefix.body,
    inboundKind: "report",
  };
  const systemPointerMessage: NormalizedChatMessage = {
    id: "parsed-system-pointer-message",
    type: "user",
    content: systemPrefix.body,
    inboundKind: "system",
  };
  const unmarkedPointerMessage: NormalizedChatMessage = {
    id: "unmarked-pointer-message",
    type: "user",
    content: "/agent-pad/pointer/opaque-unmarked-dom-15.md",
  };
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
        <MessageBubble message={tabPointerMessage} origin={tabPrefix.origin} />
        <MessageBubble message={reportPointerMessage} origin={reportPrefix.origin} />
        <MessageBubble message={systemPointerMessage} origin={systemPrefix.origin} />
        <MessageBubble message={unmarkedPointerMessage} />
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
  const tabPointer = host.querySelector<HTMLElement>('[data-testid="chat-inbound-message"]');
  const tabToggle = tabPointer?.querySelector<HTMLButtonElement>('button[aria-expanded]');
  assert(tabPointer?.textContent?.includes("Received from Auditor"), "parsed Tab sender appears in the inbound header");
  assert(tabToggle?.getAttribute("aria-expanded") === "true", "Tab card keeps its existing initial disclosure state");
  assert(tabPointer.textContent?.includes("opaque-dom-tab-12.md"), "expanded Tab card displays the opaque pointer body");
  await act(async () => tabToggle?.click());
  assert(tabToggle?.getAttribute("aria-expanded") === "false", "Tab card collapses on demand");
  assert(tabPointer?.textContent?.includes("Received from Auditor"), "collapsed Tab header keeps its supplied sender");
  await act(async () => tabToggle?.click());
  assert(tabToggle?.getAttribute("aria-expanded") === "true", "Tab card expands on demand");
  assert(tabPointer?.textContent?.includes("opaque-dom-tab-12.md"), "expanded Tab body keeps its opaque pointer");
  const reportPointer = host.querySelector<HTMLElement>('[data-testid="chat-inbound-report"]');
  const reportToggle = reportPointer?.querySelector<HTMLButtonElement>('button[aria-expanded]');
  assert(reportPointer?.textContent?.includes("Report from Worker"), "parsed report sender appears in the report header");
  assert(reportToggle?.getAttribute("aria-expanded") === "true", "report card keeps its existing initial disclosure state");
  assert(reportPointer.textContent?.includes("opaque-dom-report-13.md"), "expanded report displays the opaque pointer body");
  await act(async () => reportToggle?.click());
  assert(reportToggle?.getAttribute("aria-expanded") === "false", "report card collapses on demand");
  assert(reportPointer?.textContent?.includes("Report from Worker"), "collapsed report header keeps its supplied sender");
  await act(async () => reportToggle?.click());
  assert(reportToggle?.getAttribute("aria-expanded") === "true", "report card expands on demand");
  assert(reportPointer?.textContent?.includes("opaque-dom-report-13.md"), "expanded report body keeps its opaque pointer");
  await act(async () => {
    tabToggle?.click();
    reportToggle?.click();
  });
  assert(!tabPointer?.querySelector('[data-testid="raw-command-disclosure"]'), "collapsed Tab card hides its detail disclosure");
  assert(!reportPointer?.querySelector('[data-testid="raw-command-disclosure"]'), "collapsed report card hides its detail disclosure");
  const systemPointer = host.querySelector<HTMLElement>('[data-testid="chat-system-event"]');
  const systemPointerTitle = systemPointer?.querySelector(".ct-card__title");
  const systemPointerToggle = systemPointer?.querySelector<HTMLButtonElement>("button");
  assert(systemPointer?.parentElement?.getAttribute("role") === "status", "parsed System remains a status message");
  assert(systemPointerTitle?.textContent === "System", "duplicate System context is omitted from the event title");
  assert(systemPointerToggle?.getAttribute("aria-expanded") === "false", "System disclosure remains collapsed initially");
  assert(!systemPointer?.textContent?.includes("opaque-dom-system-14.md"), "collapsed System event hides its opaque pointer body");
  await act(async () => systemPointerToggle?.click());
  assert(systemPointerToggle?.getAttribute("aria-expanded") === "true", "System disclosure expands on demand");
  assert(systemPointer?.textContent?.includes("opaque-dom-system-14.md"), "expanded System event shows its opaque pointer body");
  assert(host.querySelectorAll('[data-testid="chat-inbound-message"]').length === 1, "unmarked pointer does not acquire a supplied-sender card");
  assert(host.textContent?.includes("opaque-unmarked-dom-15.md"), "unmarked pointer remains ordinary message content");
  assert(!systemPointer?.querySelector('[data-testid="inbound-show-tab"]'), "System event has no agent focus action");
  const system = Array.from(host.querySelectorAll<HTMLElement>('[data-testid="chat-system-event"]'))
    .find((event) => event.querySelector(".ct-card__title")?.textContent === "System · lifecycle");
  const systemToggle = system?.querySelector<HTMLButtonElement>("button");
  assert(system?.querySelector(".ct-card__title")?.textContent === "System · lifecycle", "distinct System context remains visible");
  assert(systemToggle?.getAttribute("aria-expanded") === "false", "system event should start collapsed");
  assert(!system?.textContent?.includes("Internal lifecycle detail"), "collapsed system event should hide its body");
  await act(async () => systemToggle?.click());
  assert(system?.textContent?.includes("Internal lifecycle detail"), "system event should expand on demand");
  const editDetailsId = edit?.querySelector("button")?.getAttribute("aria-controls");
  const editDetails = editDetailsId ? edit?.querySelector<HTMLElement>(`#${editDetailsId}`) : null;
  assert(editDetails?.style.maxHeight === "600px" && editDetails.style.overflowY === "auto", "expanded code output should use the 600px internal scroll cap");
  const rawDisclosures = Array.from(host.querySelectorAll<HTMLElement>('[data-testid="raw-command-disclosure"]'));
  assert(rawDisclosures.length === 2, `raw command disclosures render only inside expanded tool cards: expected 2, got ${rawDisclosures.length}`);
  assert(rawDisclosures.every((view) => view.closest('[data-testid="chat-tool-call"]')), "raw command disclosures live inside tool cards");
  assert(rawDisclosures.every((view) => !view.textContent?.includes("User formatted") && !view.textContent?.includes("Assistant formatted")), "ordinary messages have no raw command disclosure");
  const commandDisclosure = completedCommandCard?.querySelector<HTMLButtonElement>('[aria-label="Show full command"]');
  await act(async () => commandDisclosure?.click());
  const commandView = completedCommandCard?.querySelector<HTMLElement>('[data-testid="raw-command-disclosure"]');
  assert(commandView && !commandView.querySelector('[data-testid="message-raw-content"]'), "collapsed raw disclosure hides the full invocation");
  assert(commandView.textContent?.includes("clean"), "collapsed raw disclosure keeps the result visible");
  const commandRaw = Array.from(commandView?.querySelectorAll("button") ?? []).find((button) => button.textContent === "Raw command");
  await act(async () => commandRaw?.click());
  assert(commandRaw?.getAttribute("aria-expanded") === "true", "raw command disclosure expands");
  assert(commandView?.querySelector('[data-testid="message-raw-content"]')?.textContent === "git status --short", "raw command disclosure preserves the full invocation");
  assert(commandView?.textContent?.includes("clean"), "expanded raw command keeps the result visible");

  await act(async () => root.unmount());
  host.remove();
  console.log("ok - chat expansion defaults");
}

void run();
