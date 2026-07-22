import React from "react";
import { TaskCard } from "./TaskCard";
import { assertThemeTokens, event, render } from "./cardTestSetup";

const host = await render(<TaskCard event={event({ kind: "task", detail: "Build the preview", status: "pending" })} expanded onToggle={() => {}} />);
if (!host.textContent?.includes("spawn") || !host.textContent.includes("Build the preview") || !host.querySelector('[data-testid="toolcard-spinner"]')) throw new Error("task card regression");
assertThemeTokens(["--ct-magenta", "--ct-fg", "--ct-muted-fg", "--ct-border", "--ct-surface"]);
const withModel = await render(<TaskCard event={event({ kind: "task", summary: "spawn task --model gpt-5.6-luna", status: "success" })} expanded={false} onToggle={() => {}} />);
if (withModel.querySelector('[data-testid="toolcard-task-model"]')?.textContent !== "gpt-5.6-luna") throw new Error("task model chip regression");
const withoutModel = await render(<TaskCard event={event({ kind: "task", summary: "spawn task", detail: "Build the preview", status: "success" })} expanded={false} onToggle={() => {}} />);
if (withoutModel.querySelector('[data-testid="toolcard-task-model"]')) throw new Error("task model chip should be omitted");
console.log("task card tests: passed");
