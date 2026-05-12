import Graph from "graphology";
import { layoutLouvain } from "./louvainLayout";

let passed = 0;
let failed = 0;

function assert(label: string, ok: boolean, detail?: string) {
  if (ok) {
    console.log(`  ✓ ${label}`);
    passed++;
  } else {
    console.error(`  ✗ ${label}${detail ? `: ${detail}` : ""}`);
    failed++;
  }
}

function assertNoThrow(label: string, fn: () => void) {
  try {
    fn();
    console.log(`  ✓ ${label}`);
    passed++;
  } catch (e) {
    console.error(`  ✗ ${label}: threw ${e}`);
    failed++;
  }
}

// Empty graph (0 nodes, 0 edges) — must not throw
console.log("layoutLouvain: empty graph");
assertNoThrow("0 nodes 0 edges", () => {
  const g = new Graph({ multi: false });
  layoutLouvain(g);
  assert("order is still 0", g.order === 0);
});

// Nodes but 0 edges — this was the crash: inferType returns "mixed", louvain threw
console.log("\nlayoutLouvain: nodes with no edges");
assertNoThrow("3 nodes 0 edges", () => {
  const g = new Graph({ multi: false });
  g.addNode("a");
  g.addNode("b");
  g.addNode("c");
  layoutLouvain(g);
  // All nodes should have x/y attributes set
  const hasCoords = g.nodes().every((id) => {
    const x = g.getNodeAttribute(id, "x");
    const y = g.getNodeAttribute(id, "y");
    return typeof x === "number" && Number.isFinite(x) && typeof y === "number" && Number.isFinite(y);
  });
  assert("all nodes have finite x/y", hasCoords);
});

// Nodes with edges — normal path must still work
console.log("\nlayoutLouvain: nodes with edges");
assertNoThrow("3 nodes 2 edges", () => {
  const g = new Graph({ multi: false });
  g.addNode("a");
  g.addNode("b");
  g.addNode("c");
  g.addEdge("a", "b", { weight: 1.0 });
  g.addEdge("b", "c", { weight: 0.5 });
  layoutLouvain(g);
  const hasCoords = g.nodes().every((id) => {
    const x = g.getNodeAttribute(id, "x");
    const y = g.getNodeAttribute(id, "y");
    return typeof x === "number" && Number.isFinite(x) && typeof y === "number" && Number.isFinite(y);
  });
  assert("all nodes have finite x/y", hasCoords);
});

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) throw new Error(`${failed} test(s) failed`);
