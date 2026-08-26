import { act, type ReactElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { AgentLevelChoice, AppConfig } from "../../../lib/types";
import type { PushChannelDescriptor } from "../../../lib/notifyDestinations";

/** Two channels that can both carry a message. Two is the whole point: it is
 *  the roster size at which a control per channel starts multiplying rows. */
export const TELEGRAM: PushChannelDescriptor = {
  id: "telegram",
  label: "Telegram",
  control_available: true,
};

export const SLACK: PushChannelDescriptor = {
  id: "slack",
  label: "Slack",
  control_available: true,
};

/** Registered, listed by the daemon, and unable to carry anything. */
export const DISCORD: PushChannelDescriptor = {
  id: "discord",
  label: "Discord",
  control_available: false,
};

export const LEVELS: readonly string[] = ["verbose", "action_needed", "failure"];

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

export async function mount(element: ReactElement) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(element));
  return {
    host,
    async done() {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

/** Every control carrying a checked state, whichever role expresses it. */
export function controls(scope: Element): HTMLButtonElement[] {
  return Array.from(
    scope.querySelectorAll('button[role="switch"], button[role="checkbox"]'),
  ) as HTMLButtonElement[];
}

export function switches(scope: Element): HTMLButtonElement[] {
  return Array.from(scope.querySelectorAll('button[role="switch"]')) as HTMLButtonElement[];
}

export function options(scope: Element): HTMLButtonElement[] {
  return Array.from(scope.querySelectorAll('button[role="checkbox"]')) as HTMLButtonElement[];
}

export function levelRow(host: Element, level: string): Element {
  const row = host.querySelector(`[data-notification-level="${level}"]`);
  assert(row, `the ${level} row is rendered`);
  return row;
}

/** Every push-side control the level offers before anything is opened: this is
 *  the count the defect is about. */
export function pushTriggers(scope: Element): HTMLButtonElement[] {
  return Array.from(
    scope.querySelectorAll('[data-testid="settings-destination-picker-trigger"]'),
  ) as HTMLButtonElement[];
}

export async function openPush(host: Element, level: string): Promise<HTMLButtonElement> {
  const found = pushTriggers(levelRow(host, level));
  assert(found.length === 1, `the ${level} row offers one push control, got ${found.length}`);
  await act(async () => found[0].click());
  return found[0];
}

export function named(scope: Element, label: string): HTMLButtonElement {
  const control = controls(scope).find((entry) => entry.getAttribute("aria-label") === label);
  assert(control, `no control is named "${label}"`);
  return control;
}

/** A row that states every column it cares about outright, so no assertion
 *  downstream rests on what an unnamed column happens to default to. */
export function row(level: string, channels: Readonly<Record<string, boolean>> = {}): AgentLevelChoice {
  const choice: AgentLevelChoice = { level, bell: true, desktop: true };
  for (const [id, routed] of Object.entries(channels)) choice[id] = routed;
  return choice;
}

export function configWith({
  levels,
  admitted = {},
}: {
  levels: AgentLevelChoice[];
  admitted?: Readonly<Record<string, readonly string[]>>;
}): AppConfig {
  const integrations: Record<string, unknown> = {};
  for (const [id, ids] of Object.entries(admitted)) {
    integrations[`${id}_enabled`] = true;
    integrations[`${id}_allowed_users`] = [...ids];
  }
  return {
    notifications: { agent_levels: levels, channels: {}, show_bell: true },
    integrations,
  } as unknown as AppConfig;
}

/** Apply one emitted notifications patch the way the daemon would. */
export function applied(config: AppConfig, call: UpdateCall): AppConfig {
  assert(call.section === "notifications", `expected a notifications patch, got ${call.section}`);
  return {
    ...config,
    notifications: { ...config.notifications, ...call.patch },
  } as unknown as AppConfig;
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
