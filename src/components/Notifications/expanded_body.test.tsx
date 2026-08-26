// Run: npm run test:unit -- expanded_body
//
// An opened message must be readable: bounded and scrolling inside its own
// region, painted at content contrast from a token, reachable by keyboard, and
// given more room by the panel it lives in. Every assertion reads the style the
// element actually renders, never a class name, so a rename cannot silently
// retire the guarantee.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { Notification } from "../../lib/types";
import { NotificationPanel } from "./NotificationPanel";

let checked = 0;

function assert(condition: unknown, message: string): asserts condition {
  checked++;
  if (!condition) throw new Error(message);
}

const window = new Window({ url: "http://localhost/" });
Object.defineProperty(globalThis, "window", { value: window, configurable: true });
Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
Object.defineProperty(globalThis, "PointerEvent", { value: window.PointerEvent, configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const long: Notification = {
  id: "notice-long",
  tab_id: "tab-tab-1",
  workspace_id: "workspace-1",
  title: "Migration finished",
  body: Array.from({ length: 60 }, (_, line) => `line ${line}: the body keeps going`).join("\n"),
  severity: "Info",
  timestamp: 1_700_000_000_000,
  read: false,
};

const host = document.createElement("div");
document.body.appendChild(host);
const root = createRoot(host);

async function render() {
  await act(async () => {
    root.render(
      <NotificationPanel
        notifications={[long]}
        onMarkRead={() => {}}
        onMarkAllRead={() => {}}
        onDelete={() => {}}
        onClearAll={() => {}}
        onJumpToTab={() => {}}
        onClose={() => {}}
      />,
    );
  });
  return host.querySelector('[data-testid="notification-panel"]') as HTMLElement;
}

/** The inline declarations an element actually renders, keyed by property. */
function declarations(element: Element): Map<string, string> {
  const map = new Map<string, string>();
  for (const chunk of (element.getAttribute("style") ?? "").split(";")) {
    const split = chunk.indexOf(":");
    if (split === -1) continue;
    map.set(chunk.slice(0, split).trim().toLowerCase(), chunk.slice(split + 1).trim());
  }
  return map;
}

/** The largest pixel length a CSS length expression names. */
function widestPx(value: string): number {
  const found = value.match(/(\d+(?:\.\d+)?)px/g) ?? [];
  return found.reduce((widest, raw) => Math.max(widest, Number.parseFloat(raw)), 0);
}

function toggle(panel: HTMLElement): HTMLElement {
  const control = Array.from(panel.querySelectorAll("button")).find((button) =>
    (button.getAttribute("aria-label") ?? "").includes("complete message"),
  );
  if (!control) throw new Error("the expand control is missing");
  return control as HTMLElement;
}

function bodyRegion(panel: HTMLElement): HTMLElement | null {
  return panel.querySelector<HTMLElement>('[data-testid="notification-expanded-body"]');
}

async function run() {
  const panel = await render();

  const collapsed = declarations(panel).get("width") ?? "";
  assert(collapsed !== "", "the panel renders its own width, so a widening is observable");
  assert(bodyRegion(panel) === null, "a collapsed card renders no body region");

  await act(async () => toggle(panel).click());

  const body = bodyRegion(panel);
  assert(body, "expanding puts the body in a region of its own");
  const style = declarations(body);

  // BOUNDED AND SCROLLING: the region carries its own limit and its own
  // scroller, so a long message stays inside the card instead of growing the
  // list and pushing the rest of it away.
  assert(
    style.get("overflow-y") === "auto",
    "the body region scrolls itself rather than pushing the list",
  );
  const bound = style.get("max-height") ?? "";
  assert(
    bound !== "" && bound !== "none",
    "the body region is bounded, so a long message cannot grow the row without limit",
  );

  // CONTENT CONTRAST, FROM A TOKEN: a theme variable, never a literal, and
  // never the muted pairing the surrounding metadata uses.
  const colour = style.get("color") ?? "";
  assert(/^var\(--ct-[a-z-]+\)$/.test(colour), `the body colour is a design token, got ${colour}`);
  assert(!/#[0-9a-fA-F]{3,8}/.test(colour), "the body colour is never a bare literal");
  assert(
    !colour.includes("muted"),
    "the body reads at content contrast, not at the metadata contrast of a label",
  );

  // KEYBOARD: reachable, named, and never a trap.
  assert(body.tabIndex === 0, "the scroll region is reachable by keyboard");
  assert(
    body.getAttribute("role") === "region" &&
      (body.getAttribute("aria-label") ?? "").length > 0,
    "the scroll region announces itself as a named region",
  );
  await act(async () => body.focus());
  assert(document.activeElement === body, "the scroll region takes focus");
  const onward = Array.from(panel.querySelectorAll("button")).find((button) =>
    (button.getAttribute("aria-label") ?? "").startsWith("Delete"),
  ) as HTMLElement;
  assert(onward, "the card still offers a control after the body");
  await act(async () => onward.focus());
  assert(
    document.activeElement === onward,
    "focus leaves the scroll region freely — it never traps",
  );

  // ROOM: the panel is what widens, and only while something is open.
  const expanded = declarations(panel).get("width") ?? "";
  assert(
    widestPx(expanded) > widestPx(collapsed),
    `an expanded card widens the panel it lives in, got ${collapsed} then ${expanded}`,
  );
  assert(panel.getAttribute("data-expanded") === "true", "the panel reports something is open");

  await act(async () => toggle(panel).click());
  assert(
    (declarations(panel).get("width") ?? "") === collapsed &&
      panel.getAttribute("data-expanded") === "false",
    "collapsing returns the panel to its resting width",
  );

  await act(async () => root.unmount());
  console.log(`expanded_body tests: ${checked} assertions passed`);
}

void run();
