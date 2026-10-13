import assert from "node:assert/strict";
import { thoughtPresentation } from "./thoughtModel";

assert.deepEqual(thoughtPresentation(" \n **Inspect**\n\nDetails beyond the preview"), { headline: "Inspect", expandable: true });
assert.deepEqual(thoughtPresentation("A complete summary…\n\n"), { headline: "A complete summary…", expandable: false });
assert.deepEqual(thoughtPresentation("Single line ".repeat(80)), { headline: "Single line ".repeat(80).trim(), expandable: false });
assert.deepEqual(thoughtPresentation("\r\n**Plan**\r\nFull body"), { headline: "Plan", expandable: true });
assert.deepEqual(thoughtPresentation(" \n **  **"), { headline: "", expandable: false });
console.log("thoughtModel: 5 assertions passed");
