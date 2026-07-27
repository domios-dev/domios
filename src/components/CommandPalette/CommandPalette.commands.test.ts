// Run: npx tsx src/components/CommandPalette/CommandPalette.commands.test.ts

import { buildDefaultCommands } from "./CommandPalette";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

let newTabCalls = 0;
let closeTabCalls = 0;
const commands = buildDefaultCommands({
  newTab: () => {
    newTabCalls += 1;
  },
  closeTab: () => {
    closeTabCalls += 1;
  },
  newWorkspace: () => {},
  openSettings: () => {},
  toggleSidebar: () => {},
  spawnClaude: () => {},
});

const newTab = commands.find((command) => command.id === "new-tab");
assert(newTab, "New Tab command exists");
newTab.action();
assert(newTabCalls === 1, `New Tab action called ${newTabCalls} times`);
assert(closeTabCalls === 0, "New Tab action does not close a Tab");

const closeTab = commands.find((command) => command.id === "close-tab");
assert(closeTab, "Close Tab command exists");
closeTab.action();
assert(closeTabCalls === 1, `Close Tab action called ${closeTabCalls} times`);
assert(newTabCalls === 1, "Close Tab action does not create another Tab");

console.log("CommandPalette Tab action invocation contracts passed");
