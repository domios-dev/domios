/**
 * Run: npm run test:unit -- notify_permission
 *
 * A switch never lies. The rendered control is asserted against the permission
 * the system actually gave, not against the stored config alone: a toggle that
 * reads on while the OS refuses is the same class of defect as a send whose
 * Result was dropped, so a refusal wins over what the config remembers.
 */
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { __setInvokeForTest } from "../../lib/bridge";
import { NotificationSurfaces } from "./NotificationSurfaces";
import type { AppConfig } from "../../lib/types";

let checked = 0;
function check(condition: unknown, message: string): asserts condition {
  checked++;
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/" });
  for (const key of [
    "window",
    "document",
    "navigator",
    "HTMLElement",
    "HTMLInputElement",
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

setupDom();

interface InvokeCall {
  command: string;
  args: Record<string, unknown> | undefined;
}
interface UpdateCall {
  section: string;
  patch: Record<string, unknown>;
}

const DESKTOP_SWITCH = 'button[role="switch"][aria-label="Desktop notifications"]';
const REFUSAL = '[data-testid="settings-desktop-permission-refused"]';

function configWith(desktop: boolean | undefined): AppConfig {
  return { notifications: { desktop, sound: false } } as unknown as AppConfig;
}

/** Drain the promise chain the surface awaits. Microtasks only — nothing timed. */
async function settle(): Promise<void> {
  for (let turn = 0; turn < 4; turn++) await act(async () => {});
}

/** The whole surface under one stubbed daemon, so every read is the real path. */
async function mount(config: AppConfig, answers: Record<string, string>) {
  const calls: InvokeCall[] = [];
  const updates: UpdateCall[] = [];
  __setInvokeForTest(async <T,>(command: string, args?: Record<string, unknown>) => {
    calls.push({ command, args });
    return (answers[command] ?? null) as T;
  });
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root: Root = createRoot(host);
  await act(async () =>
    root.render(
      <NotificationSurfaces
        config={config}
        onUpdate={(section, patch) =>
          updates.push({ section, patch: patch as Record<string, unknown> })
        }
      />,
    ),
  );
  await settle();
  return {
    calls,
    updates,
    reads: () => host.querySelector(DESKTOP_SWITCH)?.getAttribute("aria-checked"),
    refusalShown: () => host.querySelector(REFUSAL) !== null,
    async flip() {
      const control = host.querySelector(DESKTOP_SWITCH) as HTMLButtonElement | null;
      check(control, "the desktop surface is an accessible switch");
      await act(async () => control.click());
      await settle();
    },
    async done() {
      await act(async () => root.unmount());
      host.remove();
      __setInvokeForTest(null);
    },
  };
}

async function aRefusedPermissionLeavesTheSwitchReadingOff() {
  const view = await mount(configWith(true), { notify_permission_state: "denied" });
  check(
    view.reads() === "false",
    `the config opted in but the system refused, so the switch must read off, got ${view.reads()}`,
  );
  check(view.refusalShown(), "and the refusal is stated rather than left as a silent off");
  await view.done();
}

async function aGrantedPermissionLetsTheStoredChoiceShow() {
  const view = await mount(configWith(true), { notify_permission_state: "granted" });
  check(view.reads() === "true", `a granted surface reads on, got ${view.reads()}`);
  check(!view.refusalShown(), "with nothing refused, nothing is claimed refused");
  await view.done();
}

async function anUnaskedPermissionIsNotTreatedAsARefusal() {
  const view = await mount(configWith(true), { notify_permission_state: "prompt" });
  check(
    view.reads() === "true",
    `an unasked permission is not a refusal, so it may not flip the switch, got ${view.reads()}`,
  );
  check(!view.refusalShown(), "and it is not reported as one either");
  await view.done();
}

async function enablingAsksAndARefusalIsNeverWrittenBackAsOn() {
  const view = await mount(configWith(false), {
    notify_permission_state: "prompt",
    establish_notify_permission: "denied",
  });
  await view.flip();
  const gesture = view.calls.filter((call) => call.command === "establish_notify_permission");
  check(gesture.length === 1, `turning the surface on establishes permission once, got ${gesture.length}`);
  check(gesture[0].args?.enabling === true, "and it establishes it for the ON direction");
  check(
    view.updates.length === 0,
    `a refused surface is not stored as enabled, got ${JSON.stringify(view.updates)}`,
  );
  check(view.reads() === "false", `and the switch stays off, got ${view.reads()}`);
  check(view.refusalShown(), "the refusal becomes visible on the switch it belongs to");
  await view.done();
}

async function disablingAsksTheOsForNothingAndStillTakesEffect() {
  const view = await mount(configWith(true), {
    notify_permission_state: "granted",
    establish_notify_permission: "granted",
  });
  await view.flip();
  const gesture = view.calls.filter((call) => call.command === "establish_notify_permission");
  check(gesture.length === 1, "the OFF direction goes through the same gesture");
  check(
    gesture[0].args?.enabling === false,
    "carrying the direction, so the daemon can decline to ask for a withdrawal",
  );
  check(
    JSON.stringify(view.updates) ===
      JSON.stringify([{ section: "notifications", patch: { desktop: false } }]),
    `turning it off writes the config, got ${JSON.stringify(view.updates)}`,
  );
  await view.done();
}

async function main() {
  await aRefusedPermissionLeavesTheSwitchReadingOff();
  await aGrantedPermissionLetsTheStoredChoiceShow();
  await anUnaskedPermissionIsNotTreatedAsARefusal();
  await enablingAsksAndARefusalIsNeverWrittenBackAsOn();
  await disablingAsksTheOsForNothingAndStillTakesEffect();
  console.log(`notify_permission/switch_never_lies: ${checked} assertions passed`);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
