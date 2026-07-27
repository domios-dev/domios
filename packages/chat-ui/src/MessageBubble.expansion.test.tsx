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
    command("collapsed-pane-list", "codeterm tab list --workspace codeterm"),
  ];

  await act(async () => {
    root.render(
      <div>
        {reduceToolEvents(messages).map((item) => {
          const message = item.kind === "message" ? item.message : item.event.message;
          return <MessageBubble key={message.id} message={message} item={item.kind === "tool" ? item : undefined} />;
        })}
      </div>,
    );
  });

  const cards = Array.from(host.querySelectorAll<HTMLElement>('[data-testid="chat-tool-call"]'));
  const send = cards.find((card) => card.textContent?.includes("send"));
  const list = cards.find((card) => card.textContent?.includes("tab list"));
  assert(send?.querySelector('[aria-label="Collapse tool details"]'), "outbound message should start expanded");
  assert(list?.querySelector('[aria-label="Expand tool details"]'), "ordinary operational command should start collapsed");

  await act(async () => root.unmount());
  host.remove();
  console.log("ok - chat expansion defaults");
}

void run();
