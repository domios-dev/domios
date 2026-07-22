import React from "react";
import { CommandCard } from "./CommandCard";
import { act, assertThemeTokens, event, render } from "./cardTestSetup";

const host = await render(<CommandCard event={event({ kind: "command", summary: "run command", detail: "echo hi" })} expanded onToggle={() => {}} />);
if (!host.textContent?.includes("command") || host.textContent.includes("echo hi")) throw new Error("expanded details must keep the raw command hidden");
if (host.querySelector('[data-testid="toolcard-command-open"]')) throw new Error("per-card command opener must not render");
if (!host.querySelector("svg") || !host.querySelector('[data-testid="toolcard-spinner"]')) throw new Error("command card state regression");
assertThemeTokens(["--ct-fg", "--ct-muted-fg", "--ct-blue", "--ct-border", "--ct-surface", "--ct-green", "--ct-yellow", "--ct-red"]);
const collapsed = await render(<CommandCard event={event({ kind: "command", result: "first line\n\nlast output", phase: "result", status: "success" })} expanded={false} onToggle={() => {}} />);
const tail = collapsed.querySelector('[data-testid="toolcard-result-tail"]');
if (tail?.textContent !== "→ last output" || tail.textContent.includes("first line") || !tail.className.includes("truncate")) throw new Error("command result tail regression");
const empty = await render(<CommandCard event={event({ kind: "command", result: "  \n", phase: "result", status: "success" })} expanded={false} onToggle={() => {}} />);
if (empty.querySelector('[data-testid="toolcard-result-tail"]')) throw new Error("empty command result tail should be absent");
await act(async () => {});
console.log("command card tests: passed");
