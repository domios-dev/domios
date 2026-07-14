import React from "react";
import { TaskCard } from "./TaskCard";
import { assertThemeTokens, event, render } from "./cardTestSetup";

const host = await render(<TaskCard event={event({ kind: "task", detail: "Build the preview", status: "pending" })} expanded onToggle={() => {}} />);
if (!host.textContent?.includes("task") || !host.textContent.includes("Build the preview") || !host.querySelector('[data-testid="toolcard-spinner"]')) throw new Error("task card regression");
assertThemeTokens(["--ct-magenta", "--ct-fg", "--ct-muted-fg", "--ct-border", "--ct-surface"]);
console.log("task card tests: passed");
