import assert from "node:assert/strict";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import {
  DEFAULT_TERMINAL_FONT_PX,
  matchesMediaQuery,
  rootFontPxFor,
} from "../lib/shellBreakpoint.testdata";
import { SHELL_NARROW_MAX_PX } from "../lib/shellBreakpoint";
import { type SidebarViewport, useSidebarViewport } from "./useSidebarViewport";

const window = new Window({ url: "http://localhost/" });
Object.assign(globalThis, {
  window,
  document: window.document,
  HTMLElement: window.HTMLElement,
  requestAnimationFrame: window.requestAnimationFrame.bind(window),
  cancelAnimationFrame: window.cancelAnimationFrame.bind(window),
  IS_REACT_ACT_ENVIRONMENT: true,
});

type MediaListener = (event: MediaQueryListEvent) => void;

/** One viewport driving every subscriber, so the hook is exercised through the query it really registers. */
function installViewport(initialWidth: number, terminalFontPx: number) {
  let width = initialWidth;
  let rootFontPx = rootFontPxFor(terminalFontPx);
  const subscriptions: Array<{ query: string; listeners: Set<MediaListener> }> = [];

  (window as unknown as { matchMedia: unknown }).matchMedia = (query: string) => {
    const listeners = new Set<MediaListener>();
    subscriptions.push({ query, listeners });
    return {
      media: query,
      get matches() {
        return matchesMediaQuery(query, width, rootFontPx);
      },
      onchange: null,
      addEventListener: (_type: string, listener: MediaListener) => listeners.add(listener),
      removeEventListener: (_type: string, listener: MediaListener) => listeners.delete(listener),
      dispatchEvent: () => true,
    };
  };

  const notify = async () => {
    await act(async () => {
      for (const subscription of subscriptions) {
        for (const listener of subscription.listeners) {
          listener({
            matches: matchesMediaQuery(subscription.query, width, rootFontPx),
            media: subscription.query,
          } as MediaQueryListEvent);
        }
      }
    });
  };

  return {
    setWidth: async (next: number) => {
      width = next;
      await notify();
    },
    setTerminalFont: async (next: number) => {
      rootFontPx = rootFontPxFor(next);
      await notify();
    },
  };
}

let latest: SidebarViewport | null = null;

function Harness() {
  latest = useSidebarViewport();
  return null;
}

function state(): SidebarViewport {
  if (!latest) throw new Error("the harness never rendered");
  return latest;
}

/** The drawer reveals on the next frame; wait on the frame, never on elapsed time. */
async function flushFrame() {
  await act(async () => {
    await new Promise((resolve) => window.requestAnimationFrame(() => resolve(null)));
  });
}

async function mount() {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(createElement(Harness)));
  await flushFrame();
  return async () => {
    await act(async () => root.unmount());
    host.remove();
  };
}

const NARROW_WIDTHS = [390, 560, 639, SHELL_NARROW_MAX_PX];
const WIDE_WIDTHS = [SHELL_NARROW_MAX_PX + 1, 720, 1280];

async function assertModesAcrossBoundary(terminalFontPx: number) {
  const viewport = installViewport(1280, terminalFontPx);
  const unmount = await mount();
  const label = `font=${terminalFontPx}`;

  for (const width of NARROW_WIDTHS) {
    await viewport.setWidth(width);
    assert.equal(state().phone, true, `${label} width=${width}: the shell is not in overlay mode`);
    assert.equal(state().open, false, `${label} width=${width}: the overlay drawer opened itself`);
  }
  for (const width of WIDE_WIDTHS) {
    await viewport.setWidth(width);
    assert.equal(state().phone, false, `${label} width=${width}: the shell is not in flow`);
    assert.equal(state().open, true, `${label} width=${width}: the in-flow sidebar is hidden`);
    assert.equal(state().mounted, true, `${label} width=${width}: the in-flow sidebar unmounted`);
  }
  await unmount();
}

async function main() {
  // The boundary holds at the shipped terminal font and at both ends of the setting's range.
  for (const terminalFont of [12, DEFAULT_TERMINAL_FONT_PX, 16]) {
    await assertModesAcrossBoundary(terminalFont);
  }

  // Changing only the terminal font must not move the mode at a fixed width.
  const viewport = installViewport(600, DEFAULT_TERMINAL_FONT_PX);
  let unmount = await mount();
  assert.equal(state().phone, true, "600px is not an overlay at the default font");
  await viewport.setTerminalFont(12);
  assert.equal(state().phone, true, "600px changed mode when the terminal font shrank");
  await viewport.setTerminalFont(16);
  assert.equal(state().phone, true, "600px changed mode when the terminal font grew");
  await viewport.setWidth(700);
  assert.equal(state().phone, false, "700px is not in flow at a large terminal font");
  await viewport.setTerminalFont(12);
  assert.equal(state().phone, false, "700px changed mode when the terminal font shrank");
  await unmount();

  // Overlay close keeps the drawer mounted through its slide-out; the in-flow column leaves at once.
  installViewport(390, DEFAULT_TERMINAL_FONT_PX);
  unmount = await mount();
  await act(async () => state().toggle());
  await flushFrame();
  assert.equal(state().open, true, "the overlay drawer did not open on toggle");
  await act(async () => state().closeOnPhone());
  assert.equal(state().open, false, "closeOnPhone left the overlay drawer open");
  assert.equal(state().mounted, true, "the overlay drawer unmounted before its slide-out");
  await unmount();

  const desktop = installViewport(1280, DEFAULT_TERMINAL_FONT_PX);
  unmount = await mount();
  await act(async () => state().closeOnPhone());
  assert.equal(state().open, true, "closeOnPhone collapsed the in-flow sidebar");
  await act(async () => state().close());
  assert.equal(state().mounted, false, "the in-flow sidebar lingered after closing");
  await desktop.setWidth(390);
  assert.equal(state().phone, true, "the shell did not switch to overlay mode");
  await unmount();

  console.log("useSidebarViewport contract ok");
}

void main();
