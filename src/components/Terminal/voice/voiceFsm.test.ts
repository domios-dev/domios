import {
  voiceReducer,
  initVoiceState,
  isDragUpGesture,
  isCancelGesture,
  DRAG_UP_PX,
  DRAG_LEFT_PX,
  type VoiceState,
  type VoiceAction,
} from "./voiceFsm";

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(`FAIL: ${label}`);
}

function assertEqual<T>(actual: T, expected: T, label: string) {
  if (actual !== expected) throw new Error(`FAIL: ${label} — got ${String(actual)}, want ${String(expected)}`);
}

function apply(actions: VoiceAction[]): VoiceState {
  return actions.reduce((s, a) => voiceReducer(s, a), initVoiceState());
}

const PID = 1;
const BASE = { x: 100, y: 300 };

// ---------- geometry helpers ----------

assert(isDragUpGesture(100, 300, 100, 300 - DRAG_UP_PX - 1), "drag up over threshold detected");
assert(!isDragUpGesture(100, 300, 100, 300 - DRAG_UP_PX), "drag up at threshold not triggered (strict >)");
assert(!isDragUpGesture(100, 300, 40, 300 - 49), "drag-left dominates → not drag-up");
assert(isCancelGesture(100, 100 - DRAG_LEFT_PX - 1, false), "drag left over threshold is cancel");
assert(!isCancelGesture(100, 100 - DRAG_LEFT_PX, false), "drag left at threshold not cancel (strict >)");
assert(isCancelGesture(100, 100, true), "overCancelZone flag forces cancel");

// ---------- basic transitions ----------

{
  const s = apply([{ type: "POINTER_DOWN", pointerId: PID, ...BASE }]);
  assertEqual(s.phase, "starting", "POINTER_DOWN → starting");
  assertEqual(s.pointerId, PID, "pointerId captured");
  assertEqual(s.startX, BASE.x, "startX set");
  assertEqual(s.startY, BASE.y, "startY set");
}

{
  const s = apply([
    { type: "POINTER_DOWN", pointerId: PID, ...BASE },
    { type: "START_OK" },
  ]);
  assertEqual(s.phase, "hold", "START_OK (no drag) → hold");
  assert(!s.releasedDuringStart, "releasedDuringStart cleared");
}

{
  const s = apply([
    { type: "POINTER_DOWN", pointerId: PID, ...BASE },
    { type: "START_FAIL" },
  ]);
  assertEqual(s.phase, "idle", "START_FAIL → idle");
}

// ---------- THE BUG: drag-up-lock-then-release must STAY locked ----------

{
  const dragY = BASE.y - (DRAG_UP_PX + 12); // 60px up, well above threshold

  const sMoved = apply([
    { type: "POINTER_DOWN", pointerId: PID, ...BASE },
    { type: "START_OK" },
    { type: "POINTER_MOVE", pointerId: PID, x: BASE.x, y: dragY, overCancelZone: false },
  ]);
  assertEqual(sMoved.phase, "locked", "POINTER_MOVE up 60px from hold → locked");
  assertEqual(sMoved.cancelArmed, false, "cancelArmed cleared on lock");

  const sUp = voiceReducer(sMoved, { type: "POINTER_UP", pointerId: PID });
  assertEqual(sUp.phase, "locked", "POINTER_UP in locked is a no-op (stay locked)");
  assert(!sUp.busy, "busy NOT set on POINTER_UP in locked");
}

{
  const dragY = BASE.y - (DRAG_UP_PX + 12);

  const s = apply([
    { type: "POINTER_DOWN", pointerId: PID, ...BASE },
    { type: "POINTER_MOVE", pointerId: PID, x: BASE.x, y: dragY, overCancelZone: false },
    { type: "POINTER_UP", pointerId: PID },
    { type: "START_OK" },
  ]);
  assertEqual(s.phase, "locked", "drag-up during warmup + release → locked after START_OK");
}

{
  const dragY = BASE.y - (DRAG_UP_PX + 12);

  const s = apply([
    { type: "POINTER_DOWN", pointerId: PID, ...BASE },
    { type: "POINTER_MOVE", pointerId: PID, x: BASE.x, y: dragY, overCancelZone: false },
    { type: "START_OK" }, // pointer still held
  ]);
  assertEqual(s.phase, "locked", "drag-up during warmup (still held) → locked on START_OK");
}

// ---------- quick tap (down + up, no drag) ----------

{
  // Pointer released before START_OK, no drag → hold+busy (hook fires finishVoice)
  const s = apply([
    { type: "POINTER_DOWN", pointerId: PID, ...BASE },
    { type: "POINTER_UP", pointerId: PID },
    { type: "START_OK" },
  ]);
  assertEqual(s.phase, "hold", "quick tap: START_OK with releasedDuringStart → hold");
  assert(s.busy, "quick tap: busy=true so hook can fire finishVoice");
  assert(s.releasedDuringStart, "releasedDuringStart preserved through START_OK");
}

// ---------- cancel gesture (drag left then release) ----------

{
  const leftX = BASE.x - (DRAG_LEFT_PX + 4);

  const sMoved = apply([
    { type: "POINTER_DOWN", pointerId: PID, ...BASE },
    { type: "START_OK" },
    { type: "POINTER_MOVE", pointerId: PID, x: leftX, y: BASE.y, overCancelZone: false },
  ]);
  assertEqual(sMoved.phase, "hold", "drag left stays in hold");
  assert(sMoved.cancelArmed, "cancelArmed set on drag left");

  const sUp = voiceReducer(sMoved, { type: "POINTER_UP", pointerId: PID });
  assertEqual(sUp.phase, "hold", "POINTER_UP in hold → still hold (caller decides cancel/finish)");
  assert(sUp.busy, "busy=true signals caller to act");
  assert(sUp.cancelArmed, "cancelArmed preserved so caller can read it");
}

// ---------- cancel zone override ----------

{
  const sHold = apply([
    { type: "POINTER_DOWN", pointerId: PID, ...BASE },
    { type: "START_OK" },
    { type: "POINTER_MOVE", pointerId: PID, x: BASE.x, y: BASE.y, overCancelZone: true },
  ]);
  assert(sHold.cancelArmed, "overCancelZone flag arms cancel");
}

// ---------- normal hold + release (no cancel, no lock) → finish ----------

{
  const sUp = apply([
    { type: "POINTER_DOWN", pointerId: PID, ...BASE },
    { type: "START_OK" },
    { type: "POINTER_MOVE", pointerId: PID, x: BASE.x - 5, y: BASE.y - 5, overCancelZone: false },
    { type: "POINTER_UP", pointerId: PID },
  ]);
  assertEqual(sUp.phase, "hold", "normal release → hold+busy (hook fires finishVoice)");
  assert(!sUp.cancelArmed, "cancelArmed stays false on small move");
  assert(sUp.busy, "busy=true for finishVoice");
}

// ---------- POINTER_CANCEL in hold → locked (system interrupted) ----------

{
  const s = apply([
    { type: "POINTER_DOWN", pointerId: PID, ...BASE },
    { type: "START_OK" },
    { type: "POINTER_CANCEL", pointerId: PID },
  ]);
  assertEqual(s.phase, "locked", "POINTER_CANCEL in hold → locked (hands-free latch)");
  assert(!s.cancelArmed, "cancelArmed cleared on cancel→lock");
}

// ---------- BUSY_END transitions ----------

{
  const sBusy = apply([
    { type: "POINTER_DOWN", pointerId: PID, ...BASE },
    { type: "START_OK" },
    { type: "BUSY_START" },
  ]);
  assert(sBusy.busy, "BUSY_START sets busy");

  const sIdle = voiceReducer(sBusy, { type: "BUSY_END_IDLE" });
  assertEqual(sIdle.phase, "idle", "BUSY_END_IDLE → idle");

  const sPreview = voiceReducer(sBusy, { type: "BUSY_END_PREVIEW" });
  assertEqual(sPreview.phase, "preview", "BUSY_END_PREVIEW → preview");
  assert(!sPreview.busy, "busy cleared on BUSY_END_PREVIEW");
}

// ---------- wrong pointerId is ignored ----------

{
  const s = apply([
    { type: "POINTER_DOWN", pointerId: PID, ...BASE },
    { type: "START_OK" },
    { type: "POINTER_MOVE", pointerId: PID + 1, x: BASE.x, y: BASE.y - 100, overCancelZone: false },
  ]);
  assertEqual(s.phase, "hold", "POINTER_MOVE from wrong pointer id is ignored");
}

// ---------- POINTER_DOWN in non-idle is ignored ----------

{
  const s = apply([
    { type: "POINTER_DOWN", pointerId: PID, ...BASE },
    { type: "POINTER_DOWN", pointerId: PID + 1, x: 0, y: 0 },
  ]);
  assertEqual(s.phase, "starting", "POINTER_DOWN in non-idle is a no-op");
  assertEqual(s.pointerId, PID, "first pointerId preserved");
}

{
  const started = apply([
    { type: "POINTER_DOWN", pointerId: PID, ...BASE },
    { type: "START_OK" },
  ]);
  const duplicateStart = voiceReducer(started, { type: "START_OK" });
  assertEqual(duplicateStart.phase, "hold", "duplicate START_OK does not restart a manual capture");
  const released = voiceReducer(started, { type: "POINTER_UP", pointerId: PID });
  const duplicateRelease = voiceReducer(released, { type: "POINTER_UP", pointerId: PID });
  assertEqual(duplicateRelease.phase, "hold", "duplicate manual release keeps one finish transition");
  assert(duplicateRelease.busy, "duplicate release cannot clear the pending finish");
}

{
  const starting = voiceReducer(initVoiceState(), { type: "HANDS_FREE_START" });
  assertEqual(starting.phase, "starting", "hardware start waits for recorder readiness");
  assertEqual(voiceReducer(starting, { type: "HANDS_FREE_START" }), starting, "duplicate start is inert");
  const active = voiceReducer(starting, { type: "HANDS_FREE_READY" });
  assertEqual(active.phase, "locked", "actual recorder readiness enters hands-free recording");
  assertEqual(voiceReducer(active, { type: "HANDS_FREE_START" }), active, "active recorder cannot restart");
  const stopped = voiceReducer(voiceReducer(active, { type: "BUSY_START" }), { type: "BUSY_END_IDLE" });
  assertEqual(stopped.phase, "idle", "stopped controller allows a fresh recording");
}

console.log("voiceFsm: all tests passed");
