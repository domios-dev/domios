// Run: npx tsx packages/chat-ui/src/mermaidFullscreen.test.ts

import { mermaidSvgSize } from "./mermaidFullscreen";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const viewBox = mermaidSvgSize('<svg viewBox="0 0 400 200"></svg>');
assert(viewBox?.width === 400 && viewBox.height === 200, "viewBox provides stable diagram dimensions");

const attributes = mermaidSvgSize('<svg width="640px" height="480"></svg>');
assert(attributes?.width === 640 && attributes.height === 480, "fixed SVG dimensions are a fallback");

assert(mermaidSvgSize('<svg viewBox="0 0 0 200"></svg>') === null, "zero dimensions cannot fit");
assert(mermaidSvgSize('<svg></svg>') === null, "missing dimensions have no geometry");
assert(mermaidSvgSize('<svg viewBox="0 0 1e309 100"></svg>') === null, "non-finite dimensions have no geometry");

console.log("mermaid fullscreen geometry tests: 5/5 passed");
