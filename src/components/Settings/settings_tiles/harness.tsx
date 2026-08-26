import { act, type ReactElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest, __setListenForTest } from "../../../lib/bridge";
import type { AgentLevelChoice, AppConfig } from "../../../lib/types";
import type { PushChannelDescriptor } from "../../../lib/notifyDestinations";

export const CHANNEL: PushChannelDescriptor = {
  id: "telegram",
  label: "Telegram",
  control_available: true,
};

/** Two admitted ids, so the audience is a real question and its section is
 *  drawn. One is the case the section is deliberately absent for. */
export const ADMITTED = ["100000001", "100000002"] as const;

export function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}

export function setupDom(): void {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  for (const key of [
    "window",
    "document",
    "navigator",
    "HTMLElement",
    "HTMLInputElement",
    "HTMLTextAreaElement",
    "HTMLSelectElement",
    "HTMLButtonElement",
    "SVGElement",
    "Element",
    "Node",
    "Event",
  ]) {
    Object.defineProperty(globalThis, key, {
      value: window[key as keyof Window],
      configurable: true,
    });
  }
}

export interface UpdateCall {
  section: string;
  patch: Record<string, unknown>;
}

/** Every level the daemon ships, stated in both directions so no assertion
 *  downstream rests on what an unnamed column defaults to. */
export function levelRows(): AgentLevelChoice[] {
  return [
    { level: "verbose", bell: true, desktop: false, telegram: false },
    { level: "action_needed", bell: true, desktop: true, telegram: true },
    { level: "failure", bell: true, desktop: true, telegram: false },
  ];
}

export function configWith({
  admitted = ADMITTED as readonly string[],
  levels = levelRows(),
}: {
  admitted?: readonly string[];
  levels?: AgentLevelChoice[];
} = {}): AppConfig {
  return {
    notifications: { agent_levels: levels, channels: {}, show_bell: true, sound: false },
    integrations: { telegram_enabled: true, telegram_allowed_users: [...admitted] },
  } as unknown as AppConfig;
}

async function daemonStub(command: string): Promise<unknown> {
  if (command === "list_agent_providers") return [];
  if (command === "push_channel_roster") return { channels: [CHANNEL] };
  if (command === "notify_permission_state") return "granted";
  return null;
}

export async function withDaemon(run: () => Promise<void>): Promise<void> {
  __setInvokeForTest(async <T,>(command: string) => daemonStub(command) as Promise<T>);
  __setListenForTest(async () => () => {});
  try {
    await run();
  } finally {
    __setInvokeForTest(null);
    __setListenForTest(null);
  }
}

export async function mount(element: ReactElement) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(element));
  // Drain the permission read the surfaces await. Microtasks only, nothing timed.
  for (let turn = 0; turn < 4; turn++) await act(async () => {});
  return {
    host,
    async done() {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

export const TILE = '[data-testid="settings-switch-tile"]';
export const SECTION = '[data-testid="settings-section"]';
export const LEVEL_CARD = '[data-testid="settings-notification-level"]';

export function tiles(scope: Element): HTMLButtonElement[] {
  return Array.from(scope.querySelectorAll(TILE)) as HTMLButtonElement[];
}

export function namedTile(scope: Element, label: string): HTMLButtonElement {
  const tile = tiles(scope).find((entry) => entry.getAttribute("aria-label") === label);
  assert(tile, `no tile is named "${label}"`);
  return tile;
}

/** The header a section is labelled by, resolved the way an assistive
 *  technology would: follow aria-labelledby to the element that holds it. */
export function sectionName(section: Element): string {
  const id = section.getAttribute("aria-labelledby");
  assert(id, "a section names itself through aria-labelledby");
  const label = section.ownerDocument.getElementById(id);
  assert(label, `the section's label element ${id} exists`);
  return label.textContent ?? "";
}

export function sectionNames(scope: Element): string[] {
  return Array.from(scope.querySelectorAll(SECTION)).map(sectionName);
}

/** Everything a keyboard would stop on inside a subtree, the host excluded. */
export function focusableWithin(scope: Element): Element[] {
  return Array.from(
    scope.querySelectorAll("a[href], button, input, select, textarea, [tabindex]"),
  );
}

const cases: string[] = [];

export async function check(name: string, run: () => Promise<void> | void): Promise<void> {
  await run();
  cases.push(name);
  console.log(`  ok ${cases.length} - ${name}`);
}

export function pass(suite: string): void {
  console.log(`PASS ${suite} — ${cases.length}/${cases.length} cases`);
}

export function fail(error: unknown): void {
  console.error(error);
  process.exitCode = 1;
}
