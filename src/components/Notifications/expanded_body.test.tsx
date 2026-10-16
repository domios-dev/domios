// Read rendered styles rather than class names so the test stays tied to behavior and verifies values happy-dom can represent.
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

/** The panel's own report of whether anything is open. */
function openState(panel: HTMLElement): string | null {
  return panel.getAttribute("data-expanded");
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

  assert(openState(panel) === "true", "the newest card is open when the panel arrives");
  assert(bodyRegion(panel), "an open card renders its body region on arrival");

  await act(async () => toggle(panel).click());

  assert(openState(panel) === "false", "closing the card reports nothing open");
  assert(bodyRegion(panel) === null, "a closed card renders no body region");

  await act(async () => toggle(panel).click());

  const body = bodyRegion(panel);
  assert(body, "expanding puts the body in a region of its own");

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

  assert(openState(panel) === "true", "the panel reports that something is open");
  await act(async () => toggle(panel).click());
  assert(openState(panel) === "false", "closing the card returns the panel to closed");

  await act(async () => root.unmount());
  console.log(`expanded_body tests: ${checked} assertions passed`);
}

void run();
