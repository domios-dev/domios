import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const here = dirname(fileURLToPath(import.meta.url));
const spec = readFileSync(join(here, "chat-plan-links.spec.ts"), "utf8");
const missingBlock = spec.slice(
  spec.indexOf('const missingPlanId = "pl_missingplan";'),
  spec.indexOf('test("@golden chat-plan-link mobile'),
);

assert(missingBlock.includes("toHaveAttribute(\"data-ref-state\", \"missing\")"), "missing ref state is asserted");
assert(missingBlock.includes("toBeDisabled()"), "missing ref must be asserted disabled");
assert(!missingBlock.includes("missingLink.click("), "missing disabled ref must not be clicked");
const afterDisabled = missingBlock.slice(missingBlock.indexOf("toBeDisabled()"));
assert(!afterDisabled.includes("api.focusWorkspace("), "missing-ref oracle must not refocus workspace after disabled assertion");

console.log("✓ chat plan links missing-ref oracle avoids disabled click");
