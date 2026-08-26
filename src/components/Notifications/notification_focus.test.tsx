import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
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

const window = new Window({ url: "http://localhost/notification-focus" });
Object.defineProperty(globalThis, "window", { value: window, configurable: true });
Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
Object.defineProperty(globalThis, "navigator", { value: window.navigator, configurable: true });
Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
Object.defineProperty(globalThis, "PointerEvent", { value: window.PointerEvent, configurable: true });
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const host = document.createElement("div");
document.body.appendChild(host);
const root = createRoot(host);

function notification(overrides: Partial<Notification> = {}): Notification {
  return {
    id: "notification-current",
    tab_id: "tab-origin",
    workspace_id: "workspace-origin",
    title: "Worker needs review",
    body: "The raising tab has the evidence.",
    severity: "Warning",
    timestamp: 2_000,
    read: false,
    ...overrides,
  };
}

const calls = {
  read: [] as string[],
  jumped: [] as Array<[string, string | null]>,
};

async function render(notifications: Notification[]) {
  await act(async () => {
    root.render(
      <NotificationPanel
        key={notifications.map((notification) => notification.id).join(",")}
        notifications={notifications}
        onMarkRead={(id) => calls.read.push(id)}
        onMarkAllRead={() => {}}
        onDelete={() => {}}
        onClearAll={() => {}}
        onJumpToTab={(tabId, workspaceId) => calls.jumped.push([tabId, workspaceId])}
        onClose={() => {}}
      />,
    );
  });
  return host.querySelector<HTMLElement>('[data-testid="notification-panel"]')!;
}

function row(panel: HTMLElement, id: string): HTMLElement {
  const found = panel.querySelector<HTMLElement>(`[data-notification-id="${id}"]`);
  if (!found) throw new Error(`notification row ${id} is missing`);
  return found;
}

function disclosure(panel: HTMLElement, title: string): HTMLButtonElement {
  const found = panel.querySelector<HTMLButtonElement>(
    `[aria-label$="complete message for ${title}"]`,
  );
  if (!found) throw new Error(`disclosure for ${title} is missing`);
  return found;
}

function width(panel: HTMLElement): string {
  return panel.style.width;
}

async function bellRowFocusesRecordTab() {
  calls.read.length = 0;
  calls.jumped.length = 0;
  const current = notification();
  const panel = await render([current]);

  await act(async () => row(panel, current.id).click());

  assert(calls.read.join() === current.id, "a bell-row click marks its record read");
  assert(
    calls.jumped.length === 1
      && calls.jumped[0][0] === current.tab_id
      && calls.jumped[0][1] === current.workspace_id,
    "a bell-row click focuses the tab identity carried by that record",
  );
}

async function newestCardStartsOpenWithTruthfulAria() {
  const older = notification({ id: "notification-older", title: "Earlier alert", timestamp: 1_000 });
  const newest = notification({ id: "notification-newest", title: "New alert", timestamp: 2_000 });
  const panel = await render([older, newest]);
  const newestDisclosure = disclosure(panel, newest.title);
  const olderDisclosure = disclosure(panel, older.title);

  assert(newestDisclosure.getAttribute("aria-expanded") === "true", "the newest card starts open");
  assert(olderDisclosure.getAttribute("aria-expanded") === "false", "historical cards stay collapsed");
  assert(
    row(panel, newest.id).querySelector('[data-testid="notification-expanded-body"]'),
    "the first-render expanded state matches the visible newest body",
  );

  await act(async () => newestDisclosure.click());
  assert(newestDisclosure.getAttribute("aria-expanded") === "false", "closing reports closed to assistive technology");
  assert(
    !row(panel, newest.id).querySelector('[data-testid="notification-expanded-body"]'),
    "closing removes the visible body too",
  );
}

async function widthFollowsDefaultOpenState() {
  const current = notification();
  const panel = await render([current]);
  const control = disclosure(panel, current.title);

  assert(panel.dataset.expanded === "true" && width(panel) === "560px", "an open newest card widens the panel on arrival");
  await act(async () => control.click());
  assert(panel.dataset.expanded === "false" && width(panel) === "360px", "closing the only open card restores the compact panel");
}

async function goneTabNeverSubstitutesAnotherIdentity() {
  calls.jumped.length = 0;
  const missing = notification({ id: "notification-gone", tab_id: "tab-gone", workspace_id: "workspace-gone" });
  const panel = await render([missing]);

  await act(async () => row(panel, missing.id).click());

  assert(
    calls.jumped.length === 1 && calls.jumped[0][0] === "tab-gone",
    "a gone tab is still requested by its recorded identity, never a replacement tab",
  );
}

function nativeToastBoundaryCannotTargetATab() {
  const rootDir = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
  const source = readFileSync(resolve(rootDir, "src-tauri/src/api/os_notify.rs"), "utf8");
  const nativeEntry = source.match(/pub fn _send_os_notification\([^)]*\)[\s\S]*?\n}\n\nfn _build_notif_html/)?.[0] ?? "";

  assert(nativeEntry.includes("notif.show()"), "the native boundary hands the toast to the notification plugin");
  assert(!nativeEntry.includes("tab_id"), "the native toast boundary receives no tab identity to focus");
  assert(
    !nativeEntry.includes("on_action") && !nativeEntry.includes("on_click"),
    "the plugin hand-off exposes no activation callback that could focus a tab",
  );
}

async function run() {
  await bellRowFocusesRecordTab();
  await newestCardStartsOpenWithTruthfulAria();
  await widthFollowsDefaultOpenState();
  await goneTabNeverSubstitutesAnotherIdentity();
  nativeToastBoundaryCannotTargetATab();

  await act(async () => root.unmount());
  console.log(`notification_focus tests: 5 tests, ${checked} assertions passed`);
}

void run();
