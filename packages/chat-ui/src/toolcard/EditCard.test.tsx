import React from "react";
import { EditCard } from "./EditCard";
import { assertThemeTokens, event, render } from "./cardTestSetup";

const host = await render(<EditCard event={event({ kind: "edit", status: "success", edits: [{ path: "src/a.ts", old: "old", new: "new" }, { path: "src/b.ts", old: "x", new: "y\nz" }] })} expanded onToggle={() => {}} />);
if (host.querySelectorAll('[data-testid="toolcard-edit-file"]').length !== 2 || !host.textContent?.includes("src/a.ts +1 -1") || !host.textContent.includes("+ new")) throw new Error("edit card regression");
const padHost = await render(<EditCard event={event({ kind: "edit", status: "success", edits: [{ path: "C:\\Users\\dev\\.codeterm\\runtime\\agent-pads\\166df18d\\message.md", old: "", new: "hello" }] })} expanded onToggle={() => {}} />);
if (!padHost.textContent?.includes("AgentPad") || !padHost.textContent.includes("message.md") || padHost.textContent.includes("C:\\Users") || padHost.textContent.includes("166df18d")) throw new Error("AgentPad edits must hide runtime paths");
assertThemeTokens(["--ct-fg", "--ct-muted-fg", "--ct-green", "--ct-red", "--ct-border", "--ct-diff-del-bg", "--ct-diff-add-bg"]);
console.log("edit card tests: passed");
