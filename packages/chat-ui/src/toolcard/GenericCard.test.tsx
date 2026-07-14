import React from "react";
import { GenericCard } from "./GenericCard";
import { assertThemeTokens, event, render } from "./cardTestSetup";

const host = await render(<GenericCard event={event({ kind: "generic", result: '{"raw":"secret"}', phase: "result", status: "success" })} expanded onToggle={() => {}} />);
if (!host.textContent?.includes("tool") || host.textContent.includes('{"raw"') || !host.textContent.includes("Structured tool output")) throw new Error("generic JSON leak regression");
assertThemeTokens(["--ct-muted-fg", "--ct-fg", "--ct-border", "--ct-surface"]);
console.log("generic card tests: passed");
