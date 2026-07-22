import React from "react";
import { ReadCard } from "./ReadCard";
import { assertThemeTokens, event, render } from "./cardTestSetup";

const host = await render(<ReadCard event={event({ kind: "read", summary: "src/app.ts", detail: "src/app.ts", result: "file content", phase: "result", status: "success" })} expanded onToggle={() => {}} />);
if (!host.textContent?.includes("src/app.ts") || !host.textContent.includes("file content")) throw new Error("read card regression");
assertThemeTokens(["--ct-fg", "--ct-muted-fg", "--ct-border", "--ct-surface"]);
console.log("read card tests: passed");
