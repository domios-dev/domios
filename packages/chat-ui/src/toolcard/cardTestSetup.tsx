import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { ToolEvent } from "@codeterm/chat-core";
import { DEFAULT_THEME, themeToCSS } from "../../../../src/lib/themeCss";

const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({ window, document: window.document, navigator: window.navigator, HTMLElement: window.HTMLElement, Element: window.Element, Node: window.Node })) {
  Object.defineProperty(globalThis, name, { configurable: true, value });
}
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

export function event(overrides: Partial<ToolEvent> = {}): ToolEvent {
  return { id: "id", phase: "call", status: "pending", kind: "generic", summary: "tool activity", message: { id: "id", type: "tool_call", content: "" }, ...overrides };
}

export async function render(node: React.ReactElement) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(node));
  return host;
}

export function assertThemeTokens(variables: string[]) {
  const theme = themeToCSS(DEFAULT_THEME);
  for (const variable of variables) {
    if (!(variable in theme)) throw new Error(`missing theme token ${variable}`);
  }
}

export { act };
