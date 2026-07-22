import React from "react";
import { GenericCard } from "./GenericCard";
import { assertThemeTokens, event, render } from "./cardTestSetup";

const host = await render(<GenericCard event={event({ kind: "generic", result: '{"raw":"secret"}', phase: "result", status: "success" })} expanded onToggle={() => {}} />);
if (host.textContent?.includes('{"raw"') || !host.textContent?.includes("Structured tool output")) throw new Error("generic JSON leak regression");
assertThemeTokens(["--ct-muted-fg", "--ct-fg", "--ct-border", "--ct-surface"]);

const imageResult = JSON.stringify([{ type: "image_url", image_url: { url: "data:image/png;base64,cmVk" } }]);
const imageHost = await render(<GenericCard event={event({ kind: "generic", result: imageResult, phase: "result", status: "success" })} expanded onToggle={() => {}} />);
if (!imageHost.querySelector('[data-testid="chat-tool-image-preview"]')) throw new Error("generic image result should render preview");
if (imageHost.textContent?.includes("cmVk")) throw new Error("generic image result leaked base64");
console.log("generic card tests: passed");
