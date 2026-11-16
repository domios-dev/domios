import assert from "node:assert/strict";
import { deriveMemoryGraphPalette } from "./memoryGraphTheme";

const dark = deriveMemoryGraphPalette("#000000", "#ffffff", "#646464");
assert.equal(dark.edgeColor(0.1), "rgb(10,10,10)");
assert.equal(dark.edgeColor(0.14), "rgb(14,14,14)");
assert.equal(dark.edgeColor(0.18), "rgb(18,18,18)");
assert.equal(dark.edgeColor(0.08), "rgb(8,8,8)");
assert.equal(dark.edgeColor(0.18, true), "rgb(9,9,9)");
assert.equal(dark.hoverEdgeColor, "rgb(115,115,115)");
assert.equal(dark.selectedEdgeColor, "rgb(166,166,166)");
assert.equal(dark.labelColor, "#ffffff");
const light = deriveMemoryGraphPalette("#ffffff", "#000000", "#646464");
assert.equal(light.edgeColor(0.1), "rgb(233,233,233)");
assert.equal(light.edgeColor(0.18), "rgb(221,221,221)");
assert.equal(light.edgeColor(0.18, true), "rgb(238,238,238)");
assert.equal(light.hoverEdgeColor, "rgb(140,140,140)");
assert.equal(light.selectedEdgeColor, "rgb(89,89,89)");
assert.equal(deriveMemoryGraphPalette("#aaaaaa", "#000000", "#646464").edgeColor(0.1), "rgb(160,160,160)");
for (const palette of [dark, light]) {
  for (const strength of [0.08, 0.1, 0.14, 0.18]) {
    assert.match(palette.edgeColor(strength), /^rgb\(\d+,\d+,\d+\)$/);
    assert.match(palette.edgeColor(strength, true), /^rgb\(\d+,\d+,\d+\)$/);
  }
}
console.log("Dark/light palette mixes, long-edge colours, opaque output and ego strengths passed");
