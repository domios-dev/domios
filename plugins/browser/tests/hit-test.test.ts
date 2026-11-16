import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it } from "node:test";
import { Window } from "happy-dom";

const source = resolve(import.meta.dirname, "../../../src-tauri/crates/codeterm-surface/src/surface/browser");
const hitTest = readFileSync(resolve(source, "hit-test.js"), "utf8");

describe("Browser pointer hit testing", () => {
  it("refuses the overlay fixture cover and accepts the uncovered target or its descendant", () => {
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
      assert.equal(check.call(target, 120, 60), "target_occluded: covered by button#cover");
      cover.remove();
      for (top of [target, label]) assert.equal(check.call(target, 120, 60), "clear");
      target.remove();
      assert.equal(check.call(target, 120, 60), "stale: detached pointer target");
    } finally { window.happyDOM.abort(); }
  });

  it("checks every shadow boundary and refuses a cover inside a closed root", () => {
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
      assert.equal(check.call(target, 10, 10), "target_occluded: covered by button#cover");
      Object.defineProperty(root, "elementFromPoint", { value: () => target });
      assert.equal(check.call(target, 10, 10), "clear");
      window.document.elementFromPoint = () => window.document.body;
      assert.equal(check.call(target, 10, 10), "target_occluded: covered by body");
      window.document.elementFromPoint = () => null;
      assert.equal(check.call(host, 10, 10), "target_occluded: no element at pointer coordinates");
    } finally { window.happyDOM.abort(); }
  });
});
