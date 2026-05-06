import { describe, it } from "node:test";
import { strict as assert } from "node:assert";
import { buildMessage } from "./composer";
import type { ProjectMapNode } from "./types";

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

describe("composer.buildMessage", () => {
  const a = node(1, "alpha", "function", "src/lib.rs", 10);
  const b = node(2, "beta", "function", "src/lib.rs", 22);
  const c = node(3, "Gamma", "class", "src/main.rs", 0);
  const nodesById = new Map<number, ProjectMapNode>([a, b, c].map((n) => [n.id, n]));

  it("empty selection → only prompt", () => {
    const out = buildMessage({
      prompt: "explore",
      selectedIds: [],
      nodesById,
      focusId: null,
      edges: [],
    });
    assert.equal(out.trim(), "explore");
  });

  it("includes selected nodes block", () => {
    const out = buildMessage({
      prompt: "do this",
      selectedIds: [1, 2],
      nodesById,
      focusId: null,
      edges: [],
    });
    assert.match(out, /Selected nodes \(2\)/);
    assert.match(out, /src\/lib\.rs:11 — alpha \(function\)/);
    assert.match(out, /src\/lib\.rs:23 — beta \(function\)/);
  });

  it("appends focus context only when focus is outside selection", () => {
    const focused = buildMessage({
      prompt: "x",
      selectedIds: [1],
      nodesById,
      focusId: 3,
      edges: [],
    });
    assert.match(focused, /Focus context/);
    const not_focused = buildMessage({
      prompt: "x",
      selectedIds: [1],
      nodesById,
      focusId: 1,
      edges: [],
    });
    assert.doesNotMatch(not_focused, /Focus context/);
  });

  it("summarises edges between selection", () => {
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
    assert.match(out, /Edges between selection: 1 invoke, 1 contain/);
  });
});
