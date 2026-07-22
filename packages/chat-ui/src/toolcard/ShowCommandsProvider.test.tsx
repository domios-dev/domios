import React, { act } from "react";
import { createRoot } from "react-dom/client";
import type { ToolEvent } from "@codeterm/chat-core";
import { ShowCommandsProvider } from "@codeterm/chat-ui";
import { CommandCard } from "./CommandCard";

const { Window } = await import("happy-dom");
const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({ window, document: window.document, navigator: window.navigator, HTMLElement: window.HTMLElement, Element: window.Element, Node: window.Node })) {
  Object.defineProperty(globalThis, name, { configurable: true, value });
}
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const event: ToolEvent = {
  id: "visibility-oracle",
  phase: "call",
  status: "pending",
  kind: "command",
  summary: "run visibility oracle",
  detail: "echo provider rerender",
  message: { id: "visibility-oracle", type: "tool_call", content: "" },
};
const host = document.createElement("div");
document.body.append(host);
const root = createRoot(host);

await act(async () => root.render(
  <ShowCommandsProvider showCommands={false}>
    <CommandCard event={event} expanded={false} onToggle={() => {}} />
  </ShowCommandsProvider>,
));
if (host.querySelector('[data-testid="toolcard-command"]')) throw new Error("command body leaked while showCommands=false");
if (host.querySelector('[data-testid="toolcard-command-open"]')) throw new Error("per-card command opener must not render");

await act(async () => root.render(
  <ShowCommandsProvider showCommands={true}>
    <CommandCard event={event} expanded={false} onToggle={() => {}} />
  </ShowCommandsProvider>,
));
if (host.querySelector('[data-testid="toolcard-command"]')?.textContent !== event.detail) throw new Error("provider rerender did not reveal command body");

await act(async () => root.unmount());
console.log("show commands provider rerender oracle: 2/2 assertions passed");
