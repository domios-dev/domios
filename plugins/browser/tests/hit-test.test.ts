import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it } from "node:test";
import { Window } from "happy-dom";

const source = resolve(import.meta.dirname, "../../../src-tauri/crates/codeterm-surface/src/surface/browser");
const hitTest = readFileSync(resolve(source, "hit-test.js"), "utf8");

// hit-test.js collects stack facts; Rust classifies them (hit_test/tests.rs). This checks DOM collection wiring.
function stackAt(window: Window, elements: unknown[]): void {
  Object.defineProperty(window.document, "elementsFromPoint", { value: () => elements, configurable: true });
}

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

  it("collects hit facts inside nested non-landmark composer containers without losing target binding", () => {
    const window = new Window({ url: "https://example.test" });
    try {
      window.document.write(readFileSync(resolve(import.meta.dirname, "fixtures/reply-composer-card.html"), "utf8"));
      const replyBtn = window.document.getElementById("reply-btn")!;
      const replyLabel = window.document.getElementById("reply-label")!;
      const replyInner = window.document.getElementById("reply-inner")!;
      const toolbar = window.document.getElementById("toolbar")!;
      const cardOverlay = window.document.getElementById("card-overlay")!;
      const stickyFooter = window.document.getElementById("sticky-footer")!;
      const consentInput = window.document.getElementById("reply-consent")!;
      const consentLabel = window.document.getElementById("consent-label")!;
      const consentText = window.document.getElementById("consent-text")!;

      const check = window.eval(`(${hitTest})`);

      // Nested button inside non-landmark <div>/<section> containers must not throw on parentElement.contains(targetEl).
      stackAt(window, [replyLabel, replyInner, replyBtn, toolbar, window.document.body]);
      const clearFacts = check.call(replyBtn, 480, 220);
      assert.equal(typeof clearFacts, "object");
      assert.equal(topDomId(clearFacts), "reply-label");
      assert.equal(clearFacts.nodes.find((node: { id: number }) => node.id === clearFacts.target)?.domId, "reply-btn");

      // Transparent wrapper div and opaque sticky footer inside composer card collect accurate metadata.
      stackAt(window, [cardOverlay, replyBtn, toolbar, window.document.body]);
      const overlayFacts = check.call(replyBtn, 480, 220);
      assert.equal(topDomId(overlayFacts), "card-overlay");
      assert.equal(overlayFacts.nodes.find((node: { domId: string }) => node.domId === "card-overlay")?.transparent, true);

      stackAt(window, [stickyFooter, replyBtn, toolbar, window.document.body]);
      const footerFacts = check.call(replyBtn, 480, 220);
      assert.equal(topDomId(footerFacts), "sticky-footer");
      assert.equal(footerFacts.nodes.find((node: { domId: string }) => node.domId === "sticky-footer")?.transparent, false);

      // Associated <label for="..."> inside a non-landmark div must recognize targetEl.id inside register().
      stackAt(window, [consentText, consentLabel, consentInput, window.document.body]);
      const labelFacts = check.call(consentInput, 40, 180);
      assert.equal(labelFacts.nodes.find((node: { domId: string }) => node.domId === "consent-label")?.associatedLabel, true);
    } finally { window.happyDOM.abort(); }
  });
});
