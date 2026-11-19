import React from "react";
import { ReadCard } from "./ReadCard";
import { assertThemeTokens, event, render } from "./cardTestSetup";

const host = await render(<ReadCard event={event({ kind: "read", summary: "src/app.ts", detail: "src/app.ts", result: "file content", phase: "result", status: "success" })} expanded onToggle={() => {}} />);
if (!host.textContent?.includes("src/app.ts") || !host.textContent.includes("file content")) throw new Error("read card regression");
const windowsPath = "C:/Users/User/AppData/Local/Temp/domios-open131-1d8cadc0/cards-created.txt";
const windowsHost = await render(<ReadCard event={event({ kind: "read", summary: windowsPath, detail: windowsPath, status: "success" })} expanded onToggle={() => {}} />);
const windowsSummary = windowsHost.querySelector('[data-testid="toolcard-summary"]')?.textContent ?? "";
if (!windowsSummary.endsWith("domios-open131-1d8cadc0/cards-created.txt") || !windowsHost.textContent?.includes("Read")) throw new Error("Windows read path must show its filename and muted directory");
assertThemeTokens(["--ct-fg", "--ct-muted-fg", "--ct-border", "--ct-surface"]);
console.log("read card tests: passed");
