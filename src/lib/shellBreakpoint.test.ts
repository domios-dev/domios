import { sidebarDrawerLayout } from "../components/Sidebar/sidebarDrawerLayout";
import assert from "node:assert/strict";
import { SHELL_NARROW_MAX_PX, SHELL_NARROW_QUERY } from "./shellBreakpoint";
import {
  DEFAULT_TERMINAL_FONT_PX,
  matchesMediaQuery,
  rootFontPxFor,
  TAILWIND_DEFAULT_SM_QUERY,
} from "./shellBreakpoint.testdata";

// The terminal font setting the app turns into the root font size; 14 is the shipped default.
const TERMINAL_FONT_SIZES = [12, DEFAULT_TERMINAL_FONT_PX, 16];
const WIDTHS = [390, 560, 639, 640, 641, 720, 1280];

function main() {
  assert.equal(SHELL_NARROW_QUERY, `(max-width: ${SHELL_NARROW_MAX_PX}px)`);

  assert.deepEqual(sidebarDrawerLayout(false), {});
  assert.ok(Object.keys(sidebarDrawerLayout(true)).length > 0);

  for (const terminalFont of TERMINAL_FONT_SIZES) {
    const rootFontPx = rootFontPxFor(terminalFont);
    for (const width of WIDTHS) {
      const js = matchesMediaQuery(SHELL_NARROW_QUERY, width, rootFontPx);
      assert.equal(
        js,
        width <= SHELL_NARROW_MAX_PX,
        `font=${terminalFont} width=${width}: the boundary moved with the font`,
      );
    }
  }

  // Why the shell query is px: Tailwind's stock `sm` is rem, so it picks a different mode at a non-default font.
  for (const [terminalFont, width] of [[12, 600], [16, 700]] as const) {
    const rootFontPx = rootFontPxFor(terminalFont);
    const stockSmNarrow = !matchesMediaQuery(TAILWIND_DEFAULT_SM_QUERY, width, rootFontPx);
    const shellNarrow = matchesMediaQuery(SHELL_NARROW_QUERY, width, rootFontPx);
    assert.notEqual(
      stockSmNarrow,
      shellNarrow,
      `font=${terminalFont} width=${width}: stock sm no longer diverges, so this witness is stale`,
    );
  }

  console.log("shellBreakpoint contract ok");
}

main();
