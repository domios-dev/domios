import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { InboundReportCard, parseInboundReport } from "./InboundReportCard";
import { MessageBubble } from "../MessageBubble";

const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({ window, document: window.document, navigator: window.navigator, HTMLElement: window.HTMLElement, Element: window.Element, Node: window.Node })) {
  Object.defineProperty(globalThis, name, { configurable: true, value });
}
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const content = "task: pl_d17yp2/task_2iw42e\nstatus: In Review\nbranch: lane/tool-cards\nsha: 1e096cba6\ntests: 11/11 green\nSTATUS: in_review | CHANGES: Added provider matrix";
const descriptor = parseInboundReport(content);
if (descriptor.task !== "pl_d17yp2/task_2iw42e") throw new Error("report task parse regression");
if (descriptor.status !== "in_review") throw new Error("latest report status must win");
if (!descriptor.fields.some((field) => field.label === "changes" && field.value === "Added provider matrix")) throw new Error("compound report fields parse regression");

const host = document.createElement("div");
document.body.append(host);
const root = createRoot(host);
const focusCalls: number[] = [];
await act(async () => root.render(
  <InboundReportCard
    message={{ id: "report-1", type: "user", content }}
    sender="Hugo"
    expanded
    onToggle={() => {}}
    onFocusTab={() => focusCalls.push(1)}
  />,
));
if (!host.querySelector('[data-testid="chat-inbound-report"]')) throw new Error("typed report card missing");
if (!host.textContent?.includes("report · Hugo") || !host.textContent.includes("task_2iw42e") || !host.textContent.includes("1e096cba6")) throw new Error("report header regression");
if (host.querySelector('[data-testid="chat-cross-pane-bubble"]')) throw new Error("report must not contain a nested message bubble");
const focus = Array.from(host.querySelectorAll("button")).find((button) => button.textContent === "Focus Tab");
if (!focus) throw new Error("report source navigation missing");
await act(async () => (focus as HTMLButtonElement).click());
if (focusCalls.length !== 1) throw new Error("report source navigation regression");
await act(async () => root.unmount());
host.remove();

const dispatchHost = document.createElement("div");
document.body.append(dispatchHost);
const dispatchRoot = createRoot(dispatchHost);
await act(async () => dispatchRoot.render(
  <MessageBubble
    message={{ id: "report-2", type: "user", content, fromLabel: "Hugo", fromPane: "e7f14bec", inboundKind: "report" }}
  />,
));
if (!dispatchHost.querySelector('[data-testid="chat-inbound-report"]')) throw new Error("MessageBubble report dispatch regression");
if (dispatchHost.querySelector('[data-testid="chat-cross-pane-bubble"]')) throw new Error("MessageBubble routed report through generic bubble");
if (!dispatchHost.textContent?.includes("Added provider matrix")) throw new Error("inbound report must be expanded by default");
await act(async () => dispatchRoot.unmount());
dispatchHost.remove();

const sendMessage = { id: "send-1", type: "tool_call" as const, content: "", toolKind: "codeterm" as const, toolSummary: 'codeterm send --tab e3fa0754 "hello Hugo"', toolDetail: 'codeterm send --tab e3fa0754 "hello Hugo"' };
const sendHost = document.createElement("div");
document.body.append(sendHost);
const sendRoot = createRoot(sendHost);
await act(async () => sendRoot.render(
  <MessageBubble
    message={sendMessage}
    item={{ kind: "tool", event: { id: "send-1", phase: "call", status: "success", kind: "codeterm", summary: sendMessage.toolSummary, detail: sendMessage.toolDetail, message: sendMessage } }}
  />,
));
if (!sendHost.querySelector('[data-testid="codeterm-message-content"]') || !sendHost.textContent?.includes("hello Hugo")) throw new Error("send card must be expanded by default");
await act(async () => sendRoot.unmount());
sendHost.remove();
console.log("inbound report card tests: 12/12 passed");
