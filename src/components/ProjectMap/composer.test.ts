import { buildMessage } from "./composer";
import type { ProjectMapNode } from "./types";

let passed = 0;
let failed = 0;

function assert(label: string, cond: boolean, detail?: string) {
  if (cond) {
    passed++;
  } else {
    failed++;
    console.error(`FAIL: ${label}${detail ? ` — ${detail}` : ""}`);
  }
}

const node = (id: number, name: string, kind: string, path: string, line: number): ProjectMapNode => ({
  id,
  kind,
  display_name: name,
  serialized_name: `${kind}:${path}::${name}`,
  path,
  start_line: line,
  start_col: 0,
  end_line: line,
  end_col: 5,
});

const a = node(1, "alpha", "function", "src/lib.rs", 10);
const b = node(2, "beta", "function", "src/lib.rs", 22);
const c = node(3, "Gamma", "class", "src/main.rs", 0);
const nodesById = new Map<number, ProjectMapNode>([a, b, c].map((n) => [n.id, n]));

{
  const out = buildMessage({ prompt: "explore", selectedIds: [], nodesById, focusId: null, edges: [] });
  assert("empty selection → only prompt", out.trim() === "explore", out);
}
{
  const out = buildMessage({ prompt: "do this", selectedIds: [1, 2], nodesById, focusId: null, edges: [] });
  assert("selected nodes header", /Selected nodes \(2\)/.test(out));
  assert("alpha row", /src\/lib\.rs:11 — alpha \(function\)/.test(out));
  assert("beta row", /src\/lib\.rs:23 — beta \(function\)/.test(out));
}
{
  const focused = buildMessage({ prompt: "x", selectedIds: [1], nodesById, focusId: 3, edges: [] });
  assert("focus context appears when outside selection", /Focus context/.test(focused));
  const not_focused = buildMessage({ prompt: "x", selectedIds: [1], nodesById, focusId: 1, edges: [] });
  assert("focus context omitted when inside selection", !/Focus context/.test(not_focused));
}
{
  const out = buildMessage({
    prompt: "",
    selectedIds: [1, 2, 3],
    nodesById,
    focusId: null,
    edges: [
      { source: 1, target: 2, kind: "invoke", weight: 1 },
      { source: 2, target: 3, kind: "contain", weight: 1 },
      { source: 1, target: 99, kind: "invoke", weight: 1 },
    ],
  });
  assert("edge summary", /Edges between selection: 1 invoke, 1 contain/.test(out), out);
}

console.log(`composer: ${passed} passed, ${failed} failed`);
if (failed > 0) {
  // exit non-zero for tsx runner
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (globalThis as any).process?.exit?.(1);
}
