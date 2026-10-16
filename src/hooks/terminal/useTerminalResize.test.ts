
import { tabResizeChanged } from "./tabResize";

let failures = 0;
function test(name: string, fn: () => void) {
  try {
    fn();
    console.log(`  ✓ ${name}`);
  } catch (e) {
    failures++;
    console.error(`  ✗ ${name}: ${e instanceof Error ? e.message : String(e)}`);
  }
}
function assert(cond: unknown, msg: string): asserts cond {
  if (!cond) throw new Error(msg);
}

test("unchanged geometry does not refit (tab switch / pty output)", () => {
  assert(tabResizeChanged({ w: 800, h: 600 }, 800, 600) === false, "same dims must skip");
});

test("width change refits", () => {
  assert(tabResizeChanged({ w: 800, h: 600 }, 640, 600) === true, "width delta must refit");
});

test("height change refits", () => {
  assert(tabResizeChanged({ w: 800, h: 600 }, 800, 480) === true, "height delta must refit");
});

test("first real size after mount refits", () => {
  assert(tabResizeChanged({ w: 0, h: 0 }, 800, 600) === true, "initial size must refit");
});

test("collapsed/hidden Tab does not refit", () => {
  assert(tabResizeChanged({ w: 800, h: 600 }, 0, 0) === false, "sub-20 dims must skip");
  assert(tabResizeChanged({ w: 0, h: 0 }, 10, 600) === false, "sub-20 width must skip");
});

if (failures > 0) {
  console.error(`\n${failures} test(s) failed`);
  process.exit(1);
}
console.log("\nall passed");
