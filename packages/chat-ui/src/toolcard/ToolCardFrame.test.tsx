import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { ToolEvent, ToolEventKind } from "@codeterm/chat-core";
import { CommandCard } from "./CommandCard";
import { ToolCardFrame } from "./ToolCardFrame";
import { toolKindVisual } from "./toolKindVisual";
import { DEFAULT_THEME, themeToCSS } from "../../../../src/lib/themeCss";

const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({ window, document: window.document, navigator: window.navigator, HTMLElement: window.HTMLElement, Element: window.Element, Node: window.Node })) {
  Object.defineProperty(globalThis, name, { configurable: true, value });
}
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function event(overrides: Partial<ToolEvent> = {}): ToolEvent {
  return { id: "id", phase: "call", status: "pending", kind: "command", summary: "run preview", detail: "echo preview", message: { id: "id", type: "tool_call", content: "" }, ...overrides };
}

async function render(node: React.ReactElement) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(node));
  return { host, root };
}

const { host } = await render(<CommandCard event={event()} expanded={false} onToggle={() => {}} />);
if (!host.textContent?.includes("command") || !host.textContent?.includes("run preview")) throw new Error("summary row regression");
if (!host.querySelector("svg")) throw new Error("glyph regression");
if (host.querySelector('[data-testid="toolcard-command"]')) throw new Error("command should start hidden");
if (host.querySelector('[data-testid="toolcard-command-open"]')) throw new Error("per-card command opener must not render");
if (host.querySelector<HTMLElement>("[style*='border-left']")) throw new Error("left rail found");

const { host: errorHost } = await render(
  <ToolCardFrame event={event({ kind: "search", status: "error" })} expanded={true} onToggle={() => {}}>
    <span>details</span>
  </ToolCardFrame>,
);
if (!errorHost.querySelector<HTMLElement>("[style*='dashed']")) throw new Error("error border regression");
if (!errorHost.querySelector('[data-testid="toolcard-status"] svg')) throw new Error("error status regression");

const usedThemeVars = [
  "--ct-border", "--ct-surface", "--ct-muted-fg", "--ct-red", "--ct-yellow", "--ct-green",
  "--ct-blue", "--ct-cyan", "--ct-magenta", "--ct-orange", "--ct-accent",
];
const themeVars = themeToCSS(DEFAULT_THEME);
for (const variable of usedThemeVars) {
  if (!(variable in themeVars)) throw new Error(`missing theme token ${variable}`);
}
for (const kind of Object.keys(toolKindVisual) as ToolEventKind[]) {
  const variable = toolKindVisual[kind].color.match(/var\((--ct-[^)]+)\)/)?.[1];
  if (!variable || !(variable in themeVars)) throw new Error(`un-themed color for ${kind}`);
}

console.log("tool card frame tests: 5/5 passed");
