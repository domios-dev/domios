import assert from "node:assert/strict";
import { initialSidebarExpanded, partitionSidebarWorkspaces } from "./sidebarSections";

const workspace = (id: string, protectedWorkspace = false) => ({ id, protected: protectedWorkspace }) as never;
const sections = partitionSidebarWorkspaces([workspace("general", true), workspace("codeterm"), workspace("binaura")]);

assert.equal(sections.general?.id, "general", "protected workspace renders under General");
assert.deepEqual(sections.workspaces.map((item) => item.id), ["codeterm", "binaura"], "protected General is absent from Workspaces folders");
assert(initialSidebarExpanded([workspace("general", true), workspace("codeterm")], "codeterm").has("general"), "fresh General starts expanded");
console.log("sidebarSections.test.ts: ok");
