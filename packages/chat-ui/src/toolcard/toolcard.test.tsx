import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { CommandCard } from "./CommandCard";
import { EditCard } from "./EditCard";
import { GenericToolCard } from "./GenericToolCard";
import { ChatBlockHostProvider } from "../blocks/ChatBlockHost";
import type { ToolEvent } from "@codeterm/chat-core";

const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({ window, document: window.document, navigator: window.navigator, HTMLElement: window.HTMLElement, Element: window.Element, Node: window.Node })) {
  Object.defineProperty(globalThis, name, { configurable: true, value });
}
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function event(overrides: Partial<ToolEvent> = {}): ToolEvent {
  return { id: "id", phase: "call", status: "pending", kind: "command", summary: "codeterm agent reports ...", message: { id: "id", type: "tool_call", content: "" }, ...overrides };
}

async function render(node: React.ReactElement) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(node));
  return { host, root };
}

const cases: Array<[string, () => Promise<void>]> = [
  ["command hides raw JSON and shows pending spinner", async () => {
    const { host } = await render(<CommandCard event={event({ detail: "codeterm agent reports the full command" })} expanded onToggle={() => {}} />);
    if (host.querySelector('[data-testid="message-raw-content"]')) throw new Error("formatted command card leaked the full command");
    if (!host.querySelector('[data-testid="toolcard-spinner"]')) throw new Error("pending command spinner regression");
    if (host.textContent.includes('{"raw')) throw new Error("raw tool JSON leak regression");
    if (host.querySelector('[data-testid="toolcard-command-open"]')) throw new Error("per-card command opener must not render");
  }],
  ["edit shows counts and diff", async () => {
    const { host } = await render(<EditCard event={event({ kind: "edit", edits: [{ path: "src/x.ts", old: "const value = 1;", new: "const value = 2;" }] })} expanded onToggle={() => {}} />);
    const highlightedKeyword = Array.from(host.querySelectorAll("span")).some((node) => node.textContent === "const");
    if (!host.textContent?.includes("src/x.ts +1 -1") || !host.textContent.includes("- const value = 1;") || !host.textContent.includes("+ const value = 2;") || !highlightedKeyword) throw new Error("edit card regression");
  }],
  ["error renders dashed card and result", async () => {
    const { host } = await render(<GenericToolCard event={event({ kind: "search", phase: "result", status: "error", result: "RESULT" })} expanded onToggle={() => {}} />);
    if (!host.textContent?.includes("RESULT") || !host.querySelector(".ct-card--error")) throw new Error("error lifecycle regression");
  }],
  ["long output stays bounded and clipped", async () => {
    const { host } = await render(<GenericToolCard event={event({ kind: "fetch", phase: "result", status: "success", result: "line\n".repeat(500) })} expanded onToggle={() => {}} />);
    const output = host.querySelector('[data-testid="toolcard-result"] pre');
    if (!output?.textContent?.includes("line") || !output.closest("[data-bounded-message]")) throw new Error("bounded output regression");
  }],
  ["generic does not leak braces", async () => {
    const { host } = await render(<GenericToolCard event={event({ kind: "generic", detail: "plain" })} expanded onToggle={() => {}} />);
    if (host.textContent?.includes("{")) throw new Error("generic JSON leak");
  }],
  ["generic and fetch use plain inline style without Tool activity filler or redundant completed badge", async () => {
    const { host: genericHost } = await render(
      <GenericToolCard
        event={event({
          kind: "generic",
          status: "success",
          title: "Run mcp_domios_messages_check",
          summary: "",
          message: { id: "g1", type: "tool_call", content: "", toolStatus: "completed", toolName: "Run mcp_domios_messages_check" },
        })}
        expanded={false}
        onToggle={() => {}}
      />,
    );
    if (!genericHost.querySelector(".ct-command-inline")) throw new Error("generic card did not use plain inline style");
    if (genericHost.textContent?.includes("Tool activity")) throw new Error("generic card leaked Tool activity filler");
    if (genericHost.textContent?.includes("completed")) throw new Error("generic card showed redundant completed chip");

    const { host: fetchHost } = await render(
      <GenericToolCard
        event={event({ kind: "fetch", status: "success", title: "Fetch docs", summary: "" })}
        expanded={false}
        onToggle={() => {}}
      />,
    );
    if (!fetchHost.querySelector(".ct-command-inline")) throw new Error("fetch card did not use plain inline style");
  }],
  ["clickable locations open file at line and pending commands stream live output", async () => {
    const opened: Array<[string, string, number | undefined]> = [];
    const { host } = await render(
      <ChatBlockHostProvider value={{ openFileChange: (path, view, line) => { opened.push([path, view, line]); } }}>
        <CommandCard
          event={event({
            kind: "command",
            status: "pending",
            result: "line 1\nline 2",
            locations: [{ path: "/work/src/main.ts", line: 17 }],
          })}
          expanded
          onToggle={() => {}}
        />
      </ChatBlockHostProvider>,
    );
    if (!host.querySelector('[data-testid="toolcard-spinner"]')) throw new Error("pending command missing running indicator");
    if (!host.querySelector('[data-testid="toolcard-result"]')?.textContent?.includes("line 2")) throw new Error("pending command hid live streamed output");
    const locBtn = host.querySelector<HTMLButtonElement>('[data-testid="toolcard-location"]');
    if (!locBtn || locBtn.textContent !== "src/main.ts:17") throw new Error("tool location button missing");
    await act(async () => locBtn.click());
    if (opened.length !== 1 || opened[0][0] !== "/work/src/main.ts" || opened[0][2] !== 17) throw new Error("clicking tool location did not open file at line");
  }],
];

for (const [name, run] of cases) {
  await run();
  console.log(`✓ ${name}`);
}
console.log(`toolcard tests: ${cases.length}/${cases.length} passed`);
