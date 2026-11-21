// Run: npx tsx src/components/Terminal/ComposerActionButton.test.tsx

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { ComposerActionButton } from "./ComposerActionButton";
import { ComposerAttachButton } from "./ComposerAttachButton";
import { ComposerScheduleButton } from "./ComposerScheduleButton";
import type { ComposerVoiceHandle } from "./voice/useComposerVoice";
import type { LiveDictation } from "./voice/useLiveDictation";
import { QUIET_DICTATION_SAMPLE } from "./voice/dictationLevelModel";

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

function dictationHandle(overrides: Partial<LiveDictation> = {}): LiveDictation {
  let active = (overrides.phase ?? "idle") !== "idle";
  return {
    phase: "idle",
    error: null,
    setup: null,
    start: async () => { active = true; },
    stop: async () => { active = false; },
    cancel: () => { active = false; },
    finishThen: () => undefined,
    compositionEnded: () => undefined,
    dismissSetup: () => undefined,
    active: () => active,
    readLevel: () => QUIET_DICTATION_SAMPLE,
    subscribeLevel: () => () => undefined,
    ...overrides,
  };
}

async function renderButton(
  voiceUi: ComposerVoiceHandle,
  props: { dictation?: LiveDictation; spaceArming?: boolean; spacePop?: boolean; sendDisabled?: boolean; onSend?: () => void; onSchedule?: () => void } = {},
): Promise<{ button: HTMLButtonElement; host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <ComposerActionButton
        voiceUi={voiceUi}
        dictation={props.dictation}
        spaceArming={props.spaceArming}
        spacePop={props.spacePop}
        sendDisabled={props.sendDisabled ?? false}
        onSend={props.onSend ?? (() => undefined)}
        onSchedule={props.onSchedule}
      />,
    );
  });
  return { button: host.querySelector("button") as HTMLButtonElement, host, root };
}

async function renderAttachButton(props: { uploading?: boolean; onClick?: () => void } = {}): Promise<{ button: HTMLButtonElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<ComposerAttachButton uploading={props.uploading ?? false} onClick={props.onClick ?? (() => undefined)} />);
  });
  return { button: host.querySelector("button") as HTMLButtonElement, root };
}

async function renderScheduleButton(onSchedule: () => void): Promise<{ button: HTMLButtonElement; root: Root }> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(
      <ComposerScheduleButton onSchedule={onSchedule} />,
    );
  });
  return { button: host.querySelector('[data-testid="composer-schedule-button"]') as HTMLButtonElement, root };
}

test("send preserves label, callback, type, and interaction class", async () => {
  let sends = 0;
  const { button } = await renderButton(voiceHandle(), { onSend: () => { sends++; } });
  assert(button.type === "button", "composer action must not submit a form");
  assert(button.title === "Send", "send label changed");
  assert(button.dataset.testid === "composer-send-button", "send test id changed");
  await act(async () => button.dispatchEvent(new PointerEvent("pointerup", { bubbles: true })));
  assert(sends === 1, "send callback did not fire exactly once");
});

test("composer icon actions remain round and preserve their callbacks", async () => {
  let attaches = 0;
  const { button: attach } = await renderAttachButton({ onClick: () => { attaches++; } });
  await act(async () => attach.click());
  assert(attaches === 1, "attach callback did not fire exactly once");

  const { button: disabledAttach } = await renderAttachButton({ uploading: true });
  assert(disabledAttach.disabled, "uploading attach must remain disabled");

  let schedules = 0;
  const { button: schedule } = await renderScheduleButton(() => { schedules++; });
  await act(async () => schedule.click());
  assert(schedules === 1, "schedule callback did not fire exactly once");
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

test("right-clicking send opens the schedule without sending", async () => {
  let sends = 0;
  let schedules = 0;
  const { button } = await renderButton(voiceHandle(), {
    onSend: () => { sends++; },
    onSchedule: () => { schedules++; },
  });
  await act(async () => {
    button.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, cancelable: true, button: 2 }));
    button.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, cancelable: true, button: 2 }));
    button.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2 }));
  });
  assert(sends === 0, "a secondary-button release must not send");
  assert(schedules === 1, "right-click must open the schedule exactly once");
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
  const lockPill = document.querySelector('[data-testid="composer-voice-lock-pill"]');
  assert(lockPill, "hold lock affordance missing");
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
    voice: { ...voiceHandle().voice, recording: true },
    mainIsSend: false,
    voiceMode: "hold",
    voiceActive: true,
    lockFromPointerCancel: () => { locks++; },
  }));
  await act(async () => button.dispatchEvent(new PointerEvent("pointercancel", { bubbles: true })));
  assert(locks === 1, "recording pointer cancel did not lock");
});

function pointerEvent(type: string, init: Record<string, unknown>, timeStamp: number) {
  const event = new PointerEvent(type, { bubbles: true, cancelable: true, button: 0, pointerId: 1, isPrimary: true, ...init });
  Object.defineProperty(event, "timeStamp", { value: timeStamp });
  return event;
}

test("idle dictation button keeps round chrome, bloom glyphs, and tap-to-toggle without triggering send", async () => {
  const counts = { starts: 0, stops: 0, sends: 0 };
  const dictation = dictationHandle({
    start: async () => { counts.starts++; },
    stop: async () => { counts.stops++; },
  });
  const { button } = await renderButton(voiceHandle({ mainIsSend: false }), { dictation, onSend: () => { counts.sends++; } });
  assert(button.classList.contains("ct-composer-round-button"), "idle dictation button must keep round chrome");
  assert(button.classList.contains("ct-composer-voice-bubble"), "idle dictation button must keep voice bubble class");
  assert(button.querySelector(".ct-send-bloom"), "Mic <-> Send bloom missing");
  assert(button.dataset.testid === "composer-voice-button", "voice testid missing");
  assert(button.getAttribute("aria-pressed") === "false", "idle aria-pressed must be false");

  await act(async () => {
    button.dispatchEvent(pointerEvent("pointerdown", { clientX: 100, clientY: 100 }, 1000));
    button.dispatchEvent(pointerEvent("pointerup", { clientX: 100, clientY: 100 }, 1100));
  });
  const read = () => ({ ...counts });
  assert(read().starts === 1, "quick tap must start dictation");
  assert(read().stops === 0, "quick tap must stay active (tap-to-toggle)");

  const activeDictation = dictationHandle({
    phase: "listening",
    active: () => true,
    stop: async () => { counts.stops++; },
  });
  const { button: activeButton } = await renderButton(voiceHandle({ mainIsSend: true }), {
    dictation: activeDictation,
    onSend: () => { counts.sends++; },
  });
  assert(activeButton.dataset.testid === "composer-voice-button", "active dictation must stay voice stop button even with text");
  assert(activeButton.getAttribute("aria-pressed") === "true", "active dictation aria-pressed must be true");
  await act(async () => {
    activeButton.dispatchEvent(pointerEvent("pointerdown", { clientX: 100, clientY: 100 }, 2000));
    activeButton.dispatchEvent(pointerEvent("pointerup", { clientX: 100, clientY: 100 }, 2050));
  });
  assert(read().stops === 1, "tapping active dictation button must stop dictation");
  assert(read().sends === 0, "stopping dictation must not accidentally trigger send");
});

test("dictation hold stops on release, drag-up locks, and drag-left cancels", async () => {
  const counts = { starts: 0, stops: 0, cancels: 0 };
  const read = () => ({ ...counts });
  const dictation = dictationHandle({
    start: async () => { counts.starts++; },
    stop: async () => { counts.stops++; },
    cancel: () => { counts.cancels++; },
  });
  const { button } = await renderButton(voiceHandle({ mainIsSend: false }), { dictation });

  await act(async () => {
    button.dispatchEvent(pointerEvent("pointerdown", { clientX: 100, clientY: 100 }, 1000));
    button.dispatchEvent(pointerEvent("pointerup", { clientX: 100, clientY: 100 }, 1400));
  });
  assert(read().starts === 1 && read().stops === 1 && read().cancels === 0, "holding >220ms must stop on release");

  await act(async () => {
    button.dispatchEvent(pointerEvent("pointerdown", { clientX: 100, clientY: 100 }, 2000));
    button.dispatchEvent(pointerEvent("pointermove", { clientX: 100, clientY: 30 }, 2150));
    button.dispatchEvent(pointerEvent("pointerup", { clientX: 100, clientY: 30 }, 2500));
  });
  assert(read().starts === 2 && read().stops === 1 && read().cancels === 0, "dragging up must lock hands-free");

  await act(async () => {
    button.dispatchEvent(pointerEvent("pointerdown", { clientX: 100, clientY: 100 }, 3000));
    button.dispatchEvent(pointerEvent("pointermove", { clientX: 50, clientY: 100 }, 3150));
    button.dispatchEvent(pointerEvent("pointerup", { clientX: 50, clientY: 100 }, 3200));
  });
  assert(read().starts === 3 && read().stops === 1 && read().cancels === 1, "dragging left must cancel dictation");
});

test("space-hold arming and ignition pop classes apply on the round mic button", async () => {
  const { button: armingBtn } = await renderButton(voiceHandle({ mainIsSend: false }), {
    dictation: dictationHandle(),
    spaceArming: true,
  });
  assert(armingBtn.classList.contains("ct-voice-space-arming"), "arming class missing");
  assert(armingBtn.dataset.spaceArming === "true", "arming data attribute missing");

  const { button: popBtn } = await renderButton(voiceHandle({ mainIsSend: false }), {
    dictation: dictationHandle({ phase: "listening", active: () => true }),
    spacePop: true,
  });
  assert(popBtn.classList.contains("ct-voice-space-pop"), "ignition pop class missing");
  assert(popBtn.dataset.spacePop === "true", "ignition pop data attribute missing");
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
