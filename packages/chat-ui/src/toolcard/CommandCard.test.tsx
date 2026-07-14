import React from "react";
import { CommandCard } from "./CommandCard";
import { act, assertThemeTokens, event, render } from "./cardTestSetup";

const host = await render(<CommandCard event={event({ kind: "command", summary: "run command", detail: "echo hi" })} expanded onToggle={() => {}} />);
if (!host.textContent?.includes("command") || !host.textContent.includes("echo hi")) throw new Error("command card content regression");
if (!host.querySelector("svg") || !host.querySelector('[data-testid="toolcard-spinner"]')) throw new Error("command card state regression");
assertThemeTokens(["--ct-fg", "--ct-muted-fg", "--ct-blue", "--ct-border", "--ct-surface", "--ct-green", "--ct-yellow", "--ct-red"]);
await act(async () => {});
console.log("command card tests: passed");
