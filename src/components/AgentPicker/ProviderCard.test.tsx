import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { resolve } from "node:path";

const source = readFileSync(resolve("src/components/AgentPicker/ProviderCard.tsx"), "utf8");
const styles = readFileSync(resolve("src/styles/globals.css"), "utf8");

test("selected provider exposes a strong liquid-motion selection state", () => {
  assert.match(source, /aria-pressed=\{selected\}/);
  assert.match(source, /agent-provider-card ct-liquid-interactive/);
  assert.match(source, /agent-provider-card-selection/);
  assert.match(styles, /\.agent-provider-card\[data-selected="yes"\]\s*\{[^}]*translate3d\(0, -3px, 0\)/s);
  assert.match(styles, /\.agent-provider-card\[data-selected="yes"\]\s*\{[^}]*box-shadow:/s);
  assert.match(
    styles,
    /\.agent-provider-card\[data-selected="yes"\] \.agent-provider-card-selection\s*\{[^}]*opacity:\s*1[^}]*transform:\s*scale\(1\)/s,
  );
  assert.match(
    styles,
    /\.agent-provider-card-selection\s*\{[^}]*var\(--ct-duration-liquid-press\)[^}]*var\(--ct-ease-liquid-hover|--ct-ease-hover-swell\)/s,
  );
});
