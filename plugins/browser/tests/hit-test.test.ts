import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it } from "node:test";
import { Window } from "happy-dom";

const source = resolve(import.meta.dirname, "../../../src-tauri/crates/codeterm-surface/src/surface/browser");
const hitTest = readFileSync(resolve(source, "hit-test.js"), "utf8");

// hit-test.js collects stack facts; Rust classifies them (hit_test/tests.rs). This checks DOM collection wiring.
function topDomId(facts: { stack: number[]; nodes: Array<{ id: number; domId: string }> }): string | undefined {
  const top = facts.stack[0];
  return facts.nodes.find((node) => node.id === top)?.domId;
}

describe("Browser pointer hit testing", () => {
  it("collects the covering stack for the overlay fixture and refuses a detached target", () => {
    const window = new Window({ url: "https://example.test" });
    try {
      window.document.write(readFileSync(resolve(import.meta.dirname, "fixtures/occluded-pointer.html"), "utf8"));
      const target = window.document.getElementById("target")!;
      const cover = window.document.getElementById("cover")!;
      const label = window.document.getElementById("label")!;
      let top = cover;
      window.document.elementFromPoint = (x, y) => {
        assert.deepEqual([x, y], [120, 60]);
        return top;
      };
      const check = window.eval(`(${hitTest})`);
      const covered = check.call(target, 120, 60);
      assert.equal(topDomId(covered), "cover");
      assert.equal(covered.nodes.find((node: { id: number }) => node.id === covered.target)?.domId, "target");
      cover.remove();
      for (top of [target, label]) assert.equal(topDomId(check.call(target, 120, 60)), top.id);
      target.remove();
      assert.equal(check.call(target, 120, 60), "stale: detached pointer target");
    } finally { window.happyDOM.abort(); }
  });

  it("pierces a closed shadow root and refuses when nothing is at the point", () => {
    const window = new Window({ url: "https://example.test" });
    try {
      const host = window.document.createElement("div");
      window.document.body.append(host);
      const root = host.attachShadow({ mode: "closed" });
      root.innerHTML = '<button id="target">Intended</button><button id="cover">Cover</button>';
      const target = root.querySelector("#target")!;
      const cover = root.querySelector("#cover")!;
      window.document.elementFromPoint = () => host;
      Object.defineProperty(root, "elementFromPoint", { value: () => cover, configurable: true });
      const check = window.eval(`(${hitTest})`);
      assert.equal(topDomId(check.call(target, 10, 10)), "cover");
      window.document.elementFromPoint = () => null;
      assert.equal(check.call(host, 10, 10), "target_occluded: no element at pointer coordinates");
    } finally { window.happyDOM.abort(); }
  });
});
