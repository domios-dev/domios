import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { parseTabListTree, TabListCard } from "./TabListCard";

const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({ window, document: window.document, navigator: window.navigator, HTMLElement: window.HTMLElement, Element: window.Element, Node: window.Node })) Object.defineProperty(globalThis, name, { configurable: true, value });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const output = `General  ws_general\n  aaaa1111 Adele                   Working - card implementation\n    bbbb2222 review                 Idle - waiting for review\n\nResearch  ws_research\n  cccc3333 Scout                    Done - findings reported`;
const groups = parseTabListTree(output);
if (groups.length !== 2 || groups[0].name !== "General" || groups[0].rows[1]?.depth !== 1) throw new Error("workspace tree parsing regression");
if (groups[0].rows[0]?.status !== "Working · card implementation" || groups[1].rows[0]?.title !== "Scout" || groups[1].rows[0]?.status !== "Done · findings reported") throw new Error("tab title/status parsing regression");

const host = document.createElement("div");
document.body.append(host);
const root = createRoot(host);
await act(async () => root.render(<TabListCard event={{ id: "tabs", phase: "result", status: "success", kind: "codeterm", result: output, message: { id: "tabs", type: "tool_result", content: "" } }} />));
if (host.querySelectorAll('[data-testid="tab-list-workspace"]').length !== 2 || host.querySelectorAll('[data-testid="tab-list-row"]').length !== 3) throw new Error("compact tab list rendering regression");
if (!host.textContent?.includes("card implementation") || !host.textContent.includes("Research")) throw new Error("compact tab list labels regression");
console.log("tab list card tests: 3/3 passed");
