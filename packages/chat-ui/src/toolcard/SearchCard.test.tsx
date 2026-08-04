import React from "react";
import { SearchCard } from "./SearchCard";
import { assertThemeTokens, event, render } from "./cardTestSetup";

const host = await render(<SearchCard event={event({ kind: "search", summary: "search 3 matches in 2 files", result: "a\nb\nc", phase: "result", status: "success" })} expanded onToggle={() => {}} />);
if (!host.textContent?.includes("Search") || !host.textContent.includes("3 matches · 2 files")) throw new Error("search counts regression");
const chip = host.querySelector('[data-testid="toolcard-search-counts"]');
if (!chip?.className.includes("shrink-0") || chip.getAttribute("style")?.includes("#")) throw new Error("search count chip styling regression");
assertThemeTokens(["--ct-yellow", "--ct-muted-fg", "--ct-border", "--ct-surface"]);
console.log("search card tests: passed");
