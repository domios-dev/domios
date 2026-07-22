import { canvasFocusStore } from "./canvasFocusStore";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

canvasFocusStore.closeWorkspace();
canvasFocusStore.openWorkspace({ teamId: "team-1", managerPaneId: "manager-1" });

const target = canvasFocusStore.getWorkspaceTarget();
assert(target?.teamId === "team-1", "team id survives the workspace store");
assert(target?.managerPaneId === "manager-1", "manager pane survives the workspace store");

canvasFocusStore.closeWorkspace();
console.log("canvas focus store tests passed");
