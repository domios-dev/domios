import { planRefViewState } from "./planRefViewState";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

assert(
  planRefViewState("missing", false, false) === "missing",
  "missing backend refs must not be rewritten to unowned",
);
assert(
  planRefViewState("permission", false, false) === "permission",
  "permission backend refs must not be rewritten to unowned",
);
assert(
  planRefViewState("resolved", false, false) === "unowned",
  "resolved refs without an owner workspace remain workspace unavailable",
);
assert(
  planRefViewState("resolved", true, false) === "resolved",
  "resolved refs with an owner workspace stay resolved",
);
assert(
  planRefViewState("missing", true, true) === "unreachable",
  "resolver-wide unreachable state wins over cached states",
);

console.log("✓ plan ref view state preserves missing refs");
