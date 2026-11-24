import assert from "node:assert/strict";
import { describe, finish, it } from "../test/standaloneTest";
import { blendColor } from "./colors";
import { desaturateMachineColor, machineGlyphColor, machineThemeName, machineTileColors } from "./machineIdentity";
import { contrastRatio } from "./themeCss";

describe("machine glyph contrast", () => {
  it("keeps a readable accent", () => {
    assert.equal(machineGlyphColor("#58a6ff", "#0d1117", "#c9d1d9", 0.15), "#58a6ff");
  });

  it.each([
    ["#1e3467", "#121820", "#dce0e8"],
    ["#b2d5d9", "#f6f4ef", "#262b33"],
    ["#767676", "#767676", "#767676"],
  ])("keeps glyphs readable on selected and offline tiles", ([accent, background, foreground]) => {
    for (const selected of [true, false]) {
      for (const online of [true, false]) {
        const { tone, glyph } = machineTileColors(desaturateMachineColor(accent, online), background, foreground, selected);
        const opacity = online ? 1 : 0.7;
        const fill = blendColor(blendColor(background, tone, selected ? 0.1 * opacity : 0), tone, 0.15 * opacity);
        assert.ok(contrastRatio(tone, background) >= 3, `${tone} on ${background}`);
        assert.ok(contrastRatio(glyph, fill) >= 3, `${glyph} on ${fill}`);
      }
    }
  });
});

it("uses preset display names and humanizes unknown slugs", () => {
  assert.equal(machineThemeName("github-dark"), "GitHub Dark");
  assert.equal(machineThemeName("paper-ink"), "Paper & Ink");
  assert.equal(machineThemeName("amber-sky"), "Amber Sky");
  assert.equal(machineThemeName("Love Letters After Dark"), "Love Letters After Dark");
});

finish();
