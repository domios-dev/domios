import React from "react";
import { FetchCard } from "./FetchCard";
import { assertThemeTokens, event, render } from "./cardTestSetup";

const host = await render(<FetchCard event={event({ kind: "fetch", summary: "https://example.test", detail: "https://example.test", result: "response", phase: "result", status: "success" })} expanded onToggle={() => {}} />);
const output = host.querySelector('[data-testid="toolcard-result"] pre');
if (!host.textContent?.includes("https://example.test") || !output || (output as HTMLElement).style.maxHeight !== "12rem") throw new Error("fetch card regression");
assertThemeTokens(["--ct-orange", "--ct-fg", "--ct-muted-fg", "--ct-border", "--ct-surface"]);
console.log("fetch card tests: passed");
