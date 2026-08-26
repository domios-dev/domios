import { act, type ReactElement } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";
import type { AgentLevelChoice, AppConfig, PushChannelConfig } from "../../../lib/types";
import type { PushChannelDescriptor } from "../../../lib/notifyDestinations";

export const CHANNEL: PushChannelDescriptor = {
  id: "telegram",
  label: "Telegram",
  control_available: true,
};

/** Three ids the bridge admits inbound. Whether any of them is also an audience
 *  is a separate answer, which is the whole point of these fixtures. */
export const ADMITTED = ["100000001", "100000002", "100000003"] as const;

/** One of the admitted ids. A selection is a SUBSET of that set, so a binding
 *  holding one is typed by this and stays answerable about every offered id —
 *  inference would narrow it to the members it happens to start with, and the
 *  id a selection leaves out is exactly what these fixtures need to ask about. */
export type AdmittedId = (typeof ADMITTED)[number];

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

export function switches(scope: Element): HTMLButtonElement[] {
  return Array.from(scope.querySelectorAll('button[role="switch"]')) as HTMLButtonElement[];
}

/** Every control that carries a checked state, whichever role expresses it: the
 *  local surfaces are switches and the push picker's channels are checkboxes. */
export function controls(scope: Element): HTMLButtonElement[] {
  return Array.from(
    scope.querySelectorAll('button[role="switch"], button[role="checkbox"]'),
  ) as HTMLButtonElement[];
}

export function named(scope: Element, label: string): HTMLButtonElement {
  const control = controls(scope).find((entry) => entry.getAttribute("aria-label") === label);
  assert(control, `no control is named "${label}"`);
  return control;
}

/** Open one level's push picker, so the channels it offers become reachable.
 *  The push side is one control per level, so its options exist only while it
 *  is open — which is the whole point of the shape. */
export async function openPush(host: Element, level: string): Promise<void> {
  const trigger = host.querySelector(
    `[data-notification-level="${level}"] [data-testid="settings-destination-picker-trigger"]`,
  ) as HTMLButtonElement | null;
  assert(trigger, `the ${level} row offers a push picker`);
  await act(async () => trigger.click());
}

/** A row whose channel column is stated outright in both directions, so no
 *  assertion downstream rests on what an unnamed column happens to default to. */
export function row(level: string, routed: boolean): AgentLevelChoice {
  const choice: AgentLevelChoice = { level, bell: true, desktop: true };
  choice[CHANNEL.id] = routed;
  return choice;
}

export function configWith({
  admitted = ADMITTED as readonly string[],
  chosen,
  levels,
}: {
  admitted?: readonly string[];
  chosen?: readonly string[];
  levels: AgentLevelChoice[];
}): AppConfig {
  const channels: Record<string, PushChannelConfig> =
    chosen === undefined
      ? {}
      : { [CHANNEL.id]: { recipients: [...chosen], per_scope_limit: 12 } };
  return {
    notifications: { agent_levels: levels, channels, show_bell: true },
    integrations: {
      telegram_enabled: true,
      telegram_allowed_users: [...admitted],
      discord_enabled: false,
      discord_allowed_users: [],
    },
  } as unknown as AppConfig;
}

/** Apply one emitted notifications patch the way the daemon would, so the next
 *  assertion reads stored state rather than the patch that produced it. */
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
