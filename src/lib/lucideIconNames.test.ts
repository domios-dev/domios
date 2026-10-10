import assert from "node:assert/strict";
import dynamicIconImports from "lucide-react/dynamicIconImports.mjs";
import names from "./lucideIconNames.json";

assert.deepEqual(names, Object.keys(dynamicIconImports).sort(), "Run node scripts/sync-lucide-icons.mjs after updating Lucide");

console.log("lucideIconNames: passed");
