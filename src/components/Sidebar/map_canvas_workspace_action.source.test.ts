import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const source = fs.readFileSync(
  path.join(path.dirname(fileURLToPath(import.meta.url)), "WorkspaceItem.tsx"),
  "utf8",
);

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

assert(source.includes('data-testid="workspace-map-action"'), "workspace hover has a dedicated map action");
assert(source.includes("Open essence map for ${workspace.name}"), "map action has a workspace-specific accessible name");
assert(source.includes("onOpenMap"), "workspace map action invokes the map callback");
assert(source.includes("group-hover:opacity-60"), "map action follows the existing hover affordance");
