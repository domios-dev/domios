import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const sidebar = readFileSync(new URL("./Sidebar.tsx", import.meta.url), "utf8");
const styles = readFileSync(new URL("../../styles/globals.css", import.meta.url), "utf8");

assert.match(
  sidebar,
  /sidebar-scroll-region[^"\n]*overflow-y-auto[^"\n]*overflow-x-hidden/,
  "the vertical sidebar scroller clips horizontal overflow at its ownership boundary",
);
assert.match(
  styles,
  /\.sidebar-scroll-region::?-webkit-scrollbar\s*\{[^}]*width:\s*2px;[^}]*height:\s*0;/s,
  "the sidebar owns a narrow vertical-only native scrollbar",
);
assert.match(
  styles,
  /\.sidebar-scroll-region::?-webkit-scrollbar-button\s*\{[^}]*display:\s*none;/s,
  "the sidebar scrollbar does not reserve native arrow buttons",
);

console.log("sidebar overflow contract passed");
