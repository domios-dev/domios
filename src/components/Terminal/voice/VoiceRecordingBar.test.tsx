import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import React, { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import { VoicePreview } from "./VoicePreview";
import { VoiceRecordingBar } from "./VoiceRecordingBar";
import {
  DRAG_LEFT_PX,
  DRAG_UP_PX,
  initVoiceState,
  voiceReducer,
  type VoiceAction,
} from "./voiceFsm";

const window = new Window({ url: "http://localhost/" });
for (const [name, value] of Object.entries({
  window,
  document: window.document,
  navigator: window.navigator,
  HTMLElement: window.HTMLElement,
  HTMLAudioElement: window.HTMLAudioElement,
  SVGElement: window.SVGElement,
  Element: window.Element,
  Node: window.Node,
})) {
  Object.defineProperty(globalThis, name, { value, configurable: true });
}
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const globalsCss = readFileSync(new URL("../../../styles/globals.css", import.meta.url), "utf8");
const composerVoiceSource = readFileSync(new URL("./useComposerVoice.ts", import.meta.url), "utf8");
const voiceCss = globalsCss.slice(
  globalsCss.indexOf("/* ── Voice composer"),
  globalsCss.indexOf("/* Composer action bubble"),
);

function apply(actions: VoiceAction[]) {
  return actions.reduce(voiceReducer, initVoiceState());
}

async function render(ui: React.ReactElement): Promise<{ host: HTMLDivElement; root: Root }> {
  const host = document.createElement("div") as unknown as HTMLDivElement;
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(ui));
  return { host, root };
}

async function testStateMachineContract() {
  const pointerId = 7;
  const origin = { x: 100, y: 200 };

  assert.equal(DRAG_UP_PX, 48);
  assert.equal(DRAG_LEFT_PX, 36);

  const hold = apply([
    { type: "POINTER_DOWN", pointerId, ...origin },
    { type: "START_OK" },
  ]);
  assert.equal(hold.phase, "hold");

  const atLockThreshold = voiceReducer(hold, {
    type: "POINTER_MOVE",
    pointerId,
    x: origin.x,
    y: origin.y - DRAG_UP_PX,
    overCancelZone: false,
  });
  assert.equal(atLockThreshold.phase, "hold");

  const locked = voiceReducer(hold, {
    type: "POINTER_MOVE",
    pointerId,
    x: origin.x,
    y: origin.y - DRAG_UP_PX - 1,
    overCancelZone: false,
  });
  assert.equal(locked.phase, "locked");
  assert.equal(voiceReducer(locked, { type: "POINTER_UP", pointerId }), locked);

  const atCancelThreshold = voiceReducer(hold, {
    type: "POINTER_MOVE",
    pointerId,
    x: origin.x - DRAG_LEFT_PX,
    y: origin.y,
    overCancelZone: false,
  });
  assert.equal(atCancelThreshold.cancelArmed, false);

  const cancelArmed = voiceReducer(hold, {
    type: "POINTER_MOVE",
    pointerId,
    x: origin.x - DRAG_LEFT_PX - 1,
    y: origin.y,
    overCancelZone: false,
  });
  assert.equal(cancelArmed.phase, "hold");
  assert.equal(cancelArmed.cancelArmed, true);
  assert.equal(voiceReducer(cancelArmed, { type: "POINTER_UP", pointerId }).busy, true);

  const normalRelease = voiceReducer(hold, { type: "POINTER_UP", pointerId });
  assert.equal(normalRelease.phase, "hold");
  assert.equal(normalRelease.busy, true);
  assert.equal(normalRelease.cancelArmed, false);

  const warmupLock = apply([
    { type: "POINTER_DOWN", pointerId, ...origin },
    {
      type: "POINTER_MOVE",
      pointerId,
      x: origin.x,
      y: origin.y - DRAG_UP_PX - 1,
      overCancelZone: false,
    },
    { type: "POINTER_UP", pointerId },
    { type: "START_OK" },
  ]);
  assert.equal(warmupLock.phase, "locked");

  const pointerCancelled = voiceReducer(hold, { type: "POINTER_CANCEL", pointerId });
  assert.equal(pointerCancelled.phase, "locked");
  assert.equal(pointerCancelled.cancelArmed, false);

  const preview = voiceReducer(
    voiceReducer(locked, { type: "BUSY_START" }),
    { type: "BUSY_END_PREVIEW" },
  );
  assert.equal(preview.phase, "preview");
  assert.equal(preview.busy, false);

  assert.match(
    composerVoiceSource,
    /if \(shouldCancel\) void runCancelVoice\(\);\s*else void runFinishVoice\(true\);/,
  );
  assert.match(
    composerVoiceSource,
    /const hasClip = await voice\.stopForPreview\(\);\s*dispatch\(hasClip \? \{ type: "BUSY_END_PREVIEW" \}/,
  );
  assert.match(
    composerVoiceSource,
    /const paths = await voice\.commitPreview\(\);\s*dispatch\(\{ type: "BUSY_END_IDLE" \}\);\s*if \(paths\.length > 0\) onSendWithAttachments\(paths\);/,
  );
}

async function testRecordingAndLockedSemantics() {
  let cancels = 0;
  const cancelHintRef = React.createRef<HTMLDivElement>();
  const { host, root } = await render(
    <VoiceRecordingBar
      mode="hold"
      peaks={[0.2, 0.8]}
      cancelHintRef={cancelHintRef}
      onCancel={() => { cancels += 1; }}
    />,
  );

  const announcement = host.querySelector('[role="status"]');
  assert.equal(announcement?.textContent, "Recording voice note");
  assert.equal(announcement?.getAttribute("aria-live"), "polite");
  assert(host.querySelector(".ct-voice-recording"));
  const holdCancel = host.querySelector('[data-testid="composer-voice-cancel-hint"]') as HTMLElement;
  assert.equal(holdCancel.getAttribute("role"), "button");
  assert.equal(holdCancel.tabIndex, 0);
  await act(async () => {
    holdCancel.dispatchEvent(new window.PointerEvent("pointerdown", { bubbles: true }));
  });
  assert.equal(cancels, 1);

  await act(async () => root.render(
    <VoiceRecordingBar
      mode="locked"
      peaks={[0.2, 0.8]}
      cancelHintRef={cancelHintRef}
      onCancel={() => { cancels += 1; }}
    />,
  ));
  assert.equal(host.querySelector('[role="status"]')?.textContent, "Recording locked");
  assert(host.querySelector(".ct-voice-locked"));
  const lockedCancel = host.querySelector("button") as HTMLButtonElement;
  assert.equal(lockedCancel.textContent, "Cancel");
  await act(async () => {
    lockedCancel.dispatchEvent(new window.PointerEvent("pointerdown", { bubbles: true }));
  });
  assert.equal(cancels, 2);
  await act(async () => root.unmount());
}

async function testRetainedPreviewSemantics() {
  let discards = 0;
  let sends = 0;
  const data = {
    url: "blob:retained-voice-clip",
    durationSec: 4,
    peaks: [0.2, 0.8, 0.4],
    blob: new Blob(["retained voice clip"]),
    mime: "audio/webm",
  };
  const { host, root } = await render(
    <VoicePreview
      data={data}
      busy={false}
      onDiscard={() => { discards += 1; }}
      onSend={() => { sends += 1; }}
    />,
  );

  const preview = host.querySelector('[data-testid="composer-voice-preview"]');
  assert(preview?.classList.contains("ct-voice-preview"));
  assert.equal(preview?.querySelector("audio")?.getAttribute("src"), data.url);
  assert.equal(preview?.querySelectorAll("button").length, 3);
  const controls = Array.from(preview?.querySelectorAll("button") ?? []);
  assert.deepEqual(
    controls.map((button) => button.getAttribute("aria-label")),
    ["Delete recording", "Play", "Send voice note"],
  );
  assert(controls.every((button) => button.tabIndex === 0));
  assert(controls.every((button) => button.classList.contains("ct-voice-control")));

  await act(async () => {
    (host.querySelector('[data-testid="composer-voice-preview-discard"]') as HTMLButtonElement).click();
    (host.querySelector('[data-testid="composer-voice-preview-send"]') as HTMLButtonElement).click();
  });
  assert.equal(discards, 1);
  assert.equal(sends, 1);
  await act(async () => root.unmount());
}

function testVoiceMotionContract() {
  assert(!voiceCss.includes("cubic-bezier("));
  assert(!/\b\d+(?:\.\d+)?(?:ms|s)\b/.test(voiceCss));
  assert.match(
    voiceCss,
    /\.ct-voice-rec-dot\s*\{[^}]*var\(--ct-duration-recording-pulse\)[^}]*var\(--ct-ease-liquid-breathe\)[^}]*infinite/s,
  );
  assert.equal((voiceCss.match(/\binfinite\b/g) ?? []).length, 1);
  assert.match(
    voiceCss,
    /\.ct-voice-lock-pill\s*\{[^}]*ct-voice-lock-pop[^}]*var\(--ct-duration-normal\)[^}]*var\(--ct-ease-squash-jump\)[^}]*both/s,
  );
  assert(!/ct-voice-lock-pill\s*\{[^}]*infinite/s.test(voiceCss));
  assert(!globalsCss.includes("ct-voice-slide-left"));
  assert(!globalsCss.includes("ct-voice-bounce-up"));
  assert(!globalsCss.includes("ct-voice-bubble-pulse"));

  assert.match(
    voiceCss,
    /\.ct-voice-control\s*\{[^}]*var\(--ct-duration-liquid-hover\)[^}]*var\(--ct-ease-hover-swell\)/s,
  );
  assert.match(
    voiceCss,
    /\.ct-voice-control:not\(:disabled\):active\s*\{[^}]*var\(--ct-duration-liquid-press\)/s,
  );

  const reducedMotion = voiceCss.slice(voiceCss.indexOf("@media (prefers-reduced-motion: reduce)"));
  assert.match(reducedMotion, /\.ct-voice-fade-in,[^}]*\.ct-voice-lock-pill\s*\{[^}]*animation:\s*none/s);
  assert.match(reducedMotion, /\.ct-voice-recording \.ct-voice-rec-dot,[^}]*\.ct-voice-locked \.ct-voice-rec-dot\s*\{[^}]*opacity:\s*1/s);
  assert.match(reducedMotion, /\.ct-voice-preview\s*\{[^}]*opacity:\s*1;[^}]*transform:\s*none/s);
  assert.match(reducedMotion, /\.ct-voice-lock-pill\s*\{[^}]*opacity:\s*1;[^}]*transform:\s*translate\(-50%, 0\)/s);
}

async function main() {
  await testStateMachineContract();
  await testRecordingAndLockedSemantics();
  await testRetainedPreviewSemantics();
  testVoiceMotionContract();
  console.log("voice recording interaction tests: passed");
}

void main();
