import assert from "node:assert/strict";
import fs from "node:fs";

const source = fs.readFileSync(new URL("./Titlebar.tsx", import.meta.url), "utf8");
const toast = source.indexOf("<NotificationToastContainer");
const bell = source.indexOf("<NotificationBell");

assert.equal(source.includes("CanvasButton"), false, "titlebar should not expose the redundant workspace canvas button");
assert.equal(source.includes("onOpenSettingsAbout"), false, "titlebar brand should not open About");
assert.equal(source.includes('title="About CodeTerm"'), false, "titlebar brand should not advertise an action");
assert(toast >= 0, "titlebar should render notification toasts");
assert(bell >= 0, "titlebar should render the notification bell");
assert(toast < bell, "notification toasts should expand to the left without moving the bell");
assert(
  source.includes('className="flex min-w-0 flex-1 items-center justify-end h-full overflow-hidden"'),
  "titlebar notification lane should be shrinkable and clipped",
);
assert(
  source.includes('className="flex shrink-0 items-center gap-0.5 pr-1 sm:gap-1 sm:pr-1.5"'),
  "titlebar controls should retain a fixed lane beside the toast",
);

console.log("titlebar notification layout source test: passed");
