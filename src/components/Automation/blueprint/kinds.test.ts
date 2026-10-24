import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { blueprintActions, blueprintLegend, blueprintNodeKinds, blueprintSources, blueprintSteps, blueprintTriggers, irNodeKind } from "./kinds";

const fixture = JSON.parse(readFileSync(new URL("../../../../tests/fixtures/blueprint-nodes.golden.json", import.meta.url), "utf8")) as { nodes: { kind: string; type: string }[] };
const catalogTypes = (kind: string) => fixture.nodes.filter((node) => node.kind === kind).map((node) => node.type).sort();
assert.deepEqual(Object.keys(blueprintSteps).sort(), catalogTypes("step"));
assert.deepEqual(Object.keys(blueprintActions).sort(), catalogTypes("handler"));
assert.deepEqual(Object.values(blueprintTriggers).map((kind) => kind.catalogType).sort(), catalogTypes("trigger"));
assert.deepEqual(Object.values(blueprintSources).map((kind) => kind.catalogType).sort(), catalogTypes("source"));
assert.equal(new Set(blueprintLegend).size, Object.keys(blueprintNodeKinds).length);
assert.notEqual(blueprintSteps.http.icon, blueprintSteps.github.icon);
for (const kind of Object.values(blueprintNodeKinds)) assert.ok(kind.width > 0 && kind.label);

const kind = (node: Parameters<typeof irNodeKind>[0], deterministic = false) => irNodeKind(node, deterministic);
assert.equal(kind({ id: "a", uses: "http" }).type, "source");
assert.equal(kind({ id: "a", uses: "github" }).type, "source");
assert.equal(kind({ id: "a", uses: "probe" }).type, "source");
assert.equal(kind({ id: "a", uses: "convert" }).type, "code");
assert.equal(kind({ id: "a", run: { lit: "echo" } }).type, "code");
assert.deepEqual([kind({ id: "a", code: { lang: "python", src: "" } }).type, kind({ id: "a", code: { lang: "ts", src: "" } }).label], ["code", "TypeScript"]);
assert.equal(kind({ id: "a", uses: "ask" }).type, "agent");
assert.equal(kind({ id: "a", uses: "ask" }, true).type, "logic", "a pattern-only classifier is logic");
assert.equal(kind({ id: "a", uses: "filter" }).type, "logic");
assert.equal(kind({ id: "a", call: { ref: "mem:x" } }).type, "call");
for (const action of Object.keys(blueprintActions)) assert.equal(kind({ id: "a", uses: action }).type, action === "command" ? "code" : "action");
assert.equal(kind({ id: "a", uses: "toString" }).type, "action", "inherited names never match a registry entry");
console.log("Blueprint kind registry covers the canonical Rust node catalog and classifies definition nodes");
