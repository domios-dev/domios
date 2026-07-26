// Run: npx tsx src/components/Terminal/ComposerActionButton.test.tsx

import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { ComposerActionButton } from "./ComposerActionButton";
import type { ComposerVoiceHandle } from "./voice/useComposerVoice";

type TestFn = () => void | Promise<void>;
const tests: Array<[string, TestFn]> = [];
function test(name: string, fn: TestFn) { tests.push([name, fn]); }
function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function setupDom() {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const window = new Window({ url: "http://localhost/composer" });
  Object.defineProperty(globalThis, "window", { value: window, configurable: true });
  Object.defineProperty(globalThis, "document", { value: window.document, configurable: true });
  Object.defineProperty(globalThis, "HTMLElement", { value: window.HTMLElement, configurable: true });
  Object.defineProperty(globalThis, "SVGElement", { value: window.SVGElement, configurable: true });
  Object.defineProperty(globalThis, "Element", { value: window.Element, configurable: true });
  Object.defineProperty(globalThis, "Node", { value: window.Node, configurable: true });
  Object.defineProperty(globalThis, "PointerEvent", { value: window.PointerEvent, configurable: true });
  Object.defineProperty(globalThis, "MouseEvent", { value: window.MouseEvent, configurable: true });
}

function voiceHandle(overrides: Partial<ComposerVoiceHandle> = {}): ComposerVoiceHandle {
  return {
    voice: { recording: false },
    buttonRef: { current: null },
    voiceMode: null,
    voiceBusy: false,
    bubblePose: { x: 0, y: 0, sx: 1, sy: 1 },
    isHold: false,
    isLocked: false,
    voiceActive: false,
    mainIsSend: true,
    mainTitle: "Send",
    voicePressed: false,
    cancelArmed: false,
    startVoice: async () => undefined,
    moveVoice: () => undefined,
    stopToPreview: async () => undefined,
    lockFromPointerCancel: () => undefined,
    ...overrides,
  } as unknown as ComposerVoiceHandle;
}

async function renderButton(
  voiceUi: ComposerVoiceHandle,
  props: { sendDisabled?: boolean; onSend?: () => void; onSchedule?: () => void } = {},
): Promise<{ button: HTMLButtonElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <ComposerActionButton
        voiceUi={voiceUi}
        sendDisabled={props.sendDisabled ?? false}
        onSend={props.onSend ?? (() => undefined)}
        onSchedule={props.onSchedule}
      />,
    );
  });
  return { button: host.querySelector("button") as HTMLButtonElement, root };
}

test("send preserves label, callback, type, and interaction class", async () => {
  let sends = 0;
  const { button } = await renderButton(voiceHandle(), { onSend: () => { sends++; } });
  assert(button.type === "button", "composer action must not submit a form");
  assert(button.title === "Send", "send label changed");
  assert(button.dataset.testid === "composer-send-button", "send test id changed");
  assert(button.className.includes("ct-liquid-interactive"), "shared interaction class missing");
  assert(!button.className.includes("transition-all"), "transition-all must be removed");
  await act(async () => button.dispatchEvent(new PointerEvent("pointerup", { bubbles: true })));
  assert(sends === 1, "send callback did not fire exactly once");
});

test("send scheduling and disabled state remain unchanged", async () => {
  let schedules = 0;
  const { button } = await renderButton(voiceHandle(), {
    sendDisabled: true,
    onSchedule: () => { schedules++; },
  });
  assert(button.disabled, "disabled send must remain disabled");
  button.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
  assert(schedules === 1, "schedule callback did not fire");
});

test("voice hold starts recording and exposes lock affordance", async () => {
  let starts = 0;
  const voiceUi = voiceHandle({
    mainIsSend: false,
    mainTitle: "Hold to record voice note",
    isHold: true,
    voicePressed: true,
    voiceActive: true,
    voiceMode: "hold",
    startVoice: async () => { starts++; },
  });
  const { button } = await renderButton(voiceUi);
  assert(button.title === "Hold to record voice note", "voice label changed");
  assert(button.dataset.testid === "composer-voice-button", "voice test id changed");
  assert(document.querySelector('[data-testid="composer-voice-lock-pill"]'), "hold lock affordance missing");
  await act(async () => button.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })));
  assert(starts === 1, "voice start callback did not fire");
});

test("locked voice stops to preview and busy voice stays disabled", async () => {
  let stops = 0;
  const { button } = await renderButton(voiceHandle({
    mainIsSend: false,
    mainTitle: "Stop and review voice note",
    isLocked: true,
    voiceActive: true,
    voiceBusy: true,
    stopToPreview: async () => { stops++; },
  }));
  assert(button.title === "Stop and review voice note", "locked voice label changed");
  assert(button.disabled, "busy voice must remain disabled");
  button.disabled = false;
  await act(async () => button.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })));
  assert(stops === 1, "locked voice stop callback did not fire");
});

test("recording pointer cancellation keeps the lock callback", async () => {
  let locks = 0;
  const { button } = await renderButton(voiceHandle({
    voice: { recording: true },
    mainIsSend: false,
    voiceMode: "hold",
    voiceActive: true,
    lockFromPointerCancel: () => { locks++; },
  }));
  await act(async () => button.dispatchEvent(new PointerEvent("pointercancel", { bubbles: true })));
  assert(locks === 1, "recording pointer cancel did not lock");
});

async function main() {
  setupDom();
  let failed = 0;
  for (const [name, fn] of tests) {
    try {
      await fn();
      console.log(`  ✓ ${name}`);
    } catch (error) {
      failed++;
      console.error(`  ✗ ${name}: ${error instanceof Error ? error.message : error}`);
    }
  }
  if (failed > 0) process.exit(1);
  console.log(`\n✓ all ${tests.length} ComposerActionButton tests green`);
}

void main();
