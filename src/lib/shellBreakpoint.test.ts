import assert from "node:assert/strict";
import { SHELL_NARROW_MAX_PX, SHELL_NARROW_QUERY, SHELL_WIDE_QUERY } from "./shellBreakpoint";
import {
  classAttributeContaining,
  customVariantQuery,
  DEFAULT_TERMINAL_FONT_PX,
  matchesMediaQuery,
  repoFile,
  rootFontPxFor,
  TAILWIND_DEFAULT_SM_QUERY,
  widthMediaPreludes,
} from "./shellBreakpoint.testdata";

const globals = repoFile("src/styles/globals.css");
const app = repoFile("src/App.tsx");
const sidebar = repoFile("src/components/Sidebar/Sidebar.tsx");

const NARROW_VARIANT = customVariantQuery(globals, "shell-narrow");
const WIDE_VARIANT = customVariantQuery(globals, "shell-wide");

// The terminal font setting the app turns into the root font size; 14 is the shipped default.
const TERMINAL_FONT_SIZES = [12, DEFAULT_TERMINAL_FONT_PX, 16];
const WIDTHS = [390, 560, 639, 640, 641, 720, 1280];

function sidebarDrawerBlockQuery(): string {
  const blocks = widthMediaPreludes(globals);
  assert.ok(blocks.length > 0, "globals.css has no single-condition width media blocks");
  for (const prelude of blocks) {
    assert.equal(
      prelude,
      SHELL_NARROW_QUERY,
      `a hand-written shell block uses ${prelude} instead of the shell contract`,
    );
  }
  return blocks[0];
}

function main() {
  // The Tailwind variants, the hand-written CSS, and the JS matchMedia string are literally one query.
  assert.equal(SHELL_NARROW_QUERY, `(max-width: ${SHELL_NARROW_MAX_PX}px)`);
  assert.equal(NARROW_VARIANT, SHELL_NARROW_QUERY);
  assert.equal(WIDE_VARIANT, SHELL_WIDE_QUERY);
  const cssBlockQuery = sidebarDrawerBlockQuery();
  assert.equal(cssBlockQuery, SHELL_NARROW_QUERY);

  // The shell utilities are the scoped variants, never Tailwind's rem-based `sm`.
  const backdrop = classAttributeContaining(app, "ct-sidebar-backdrop");
  assert.ok(backdrop.includes("shell-wide:hidden"), "the backdrop does not use shell-wide:hidden");
  const drawer = classAttributeContaining(app, "ct-sidebar-drawer");
  for (const utility of [
    "shell-narrow:fixed",
    "shell-narrow:inset-y-0",
    "shell-narrow:left-0",
    "shell-narrow:z-40",
    "shell-narrow:bg-[var(--ct-sidebar-bg)]",
  ]) {
    assert.ok(drawer.includes(utility), `the drawer does not use ${utility}`);
  }
  const handle = classAttributeContaining(sidebar, "cursor-col-resize");
  assert.ok(handle.includes("shell-narrow:hidden"), "the resize handle does not use shell-narrow:hidden");
  for (const [name, line] of [["backdrop", backdrop], ["drawer", drawer], ["resize handle", handle]] as const) {
    assert.ok(!/(^|[\s`"'])(max-)?sm:/.test(line), `the ${name} still carries a Tailwind sm utility`);
  }

  // Reduced motion still owns both animated shell surfaces.
  const reducedMotion = /@media \(prefers-reduced-motion: reduce\) \{\s*\.ct-sidebar-backdrop,\s*\.ct-sidebar-drawer \{\s*transition: none;/.test(globals);
  assert.ok(reducedMotion, "the reduced-motion block no longer covers the sidebar backdrop and drawer");

  // No width, at any supported terminal font, resolves to different modes across the three systems.
  for (const terminalFont of TERMINAL_FONT_SIZES) {
    const rootFontPx = rootFontPxFor(terminalFont);
    for (const width of WIDTHS) {
      const js = matchesMediaQuery(SHELL_NARROW_QUERY, width, rootFontPx);
      const css = matchesMediaQuery(cssBlockQuery, width, rootFontPx);
      const utilityNarrow = matchesMediaQuery(NARROW_VARIANT, width, rootFontPx);
      const utilityWide = matchesMediaQuery(WIDE_VARIANT, width, rootFontPx);
      const label = `font=${terminalFont} width=${width}`;
      assert.equal(css, js, `${label}: CSS and the hook disagree`);
      assert.equal(utilityNarrow, js, `${label}: the shell utilities and the hook disagree`);
      assert.equal(utilityWide, !js, `${label}: shell-wide is not the complement of shell-narrow`);
      assert.equal(js, width <= SHELL_NARROW_MAX_PX, `${label}: the boundary moved with the font`);
    }
  }

  // Why the scoped variants exist: Tailwind's stock `sm` is rem, so it picks a different mode at a non-default font.
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
