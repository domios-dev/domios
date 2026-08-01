// Run: npx tsx packages/chat-ui/src/mermaidFullscreen.test.ts

import { fitMermaidScale, mermaidSvgSize } from "./mermaidFullscreen";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

const viewBox = mermaidSvgSize('<svg viewBox="0 0 400 200"></svg>');
assert(viewBox?.width === 400 && viewBox.height === 200, "viewBox provides stable diagram dimensions");

const attributes = mermaidSvgSize('<svg width="640px" height="480"></svg>');
assert(attributes?.width === 640 && attributes.height === 480, "fixed SVG dimensions are a fallback");

assert(fitMermaidScale({ width: 1000, height: 600 }, { width: 400, height: 200 }) === 2.42, "fit uses the limiting viewport axis");
assert(fitMermaidScale({ width: 100, height: 100 }, { width: 2000, height: 1000 }) === 0.35, "fit respects minimum zoom");
assert(fitMermaidScale({ width: 5000, height: 5000 }, { width: 100, height: 100 }) === 5, "fit respects maximum zoom");

console.log("mermaid fullscreen geometry tests: 5/5 passed");
