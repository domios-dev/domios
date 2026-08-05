// Run: npx tsx src/components/Sidebar/SidebarWorkspaceTabs.close.source.test.ts

import { readFileSync } from "node:fs";

const source = readFileSync(new URL("./SidebarWorkspaceTabs.tsx", import.meta.url), "utf8");

if (!source.includes("onCloseTeam={onCloseTeam}")) {
  throw new Error("SidebarWorkspaceTabs must expose human Team close without a manager Tab");
}
if (source.includes("onCloseTeam={managerTabId ? onCloseTeam : undefined}")) {
  throw new Error("human Team close must not depend on manager Tab authority");
}

console.log("SidebarWorkspaceTabs managerless close source checks passed");
