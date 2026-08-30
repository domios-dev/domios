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

console.log("titlebar notification layout source test: passed");
