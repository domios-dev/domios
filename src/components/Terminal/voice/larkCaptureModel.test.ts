import assert from "node:assert/strict";
import {
  claimCaptureOwnership,
  currentCaptureOwner,
  exactMicrophoneConstraints,
  hasExactLiveMicrophone,
  initialLarkCaptureState,
  larkCaptureTransition,
  releaseCaptureOwnership,
  resetCaptureOwnershipForTest,
  type LarkCaptureLease,
} from "./larkCaptureModel";

const status = {
  pluginId: "lark",
  granted: true,
  enabled: true,
  available: true,
  generation: 12,
};

const capture: LarkCaptureLease = {
  lease: "lease-1",
  pluginId: "lark",
  tabId: "tab-A",
  microphoneId: "mic-exact",
  generation: 12,
};

function armed() {
  return larkCaptureTransition(initialLarkCaptureState(), { type: "STATUS", status });
}

function exactTrack(deviceId: string, readyState: MediaStreamTrackState = "live") {
  return { readyState, getSettings: () => ({ deviceId }) } as MediaStreamTrack;
}

function assertExactDevices() {
  assert.deepEqual(exactMicrophoneConstraints("mic-exact"), {
    audio: { deviceId: { exact: "mic-exact" } },
  });
  const exact = { getAudioTracks: () => [exactTrack("mic-exact")] } as unknown as MediaStream;
  const foreign = { getAudioTracks: () => [exactTrack("mic-other")] } as unknown as MediaStream;
  const ended = { getAudioTracks: () => [exactTrack("mic-exact", "ended")] } as unknown as MediaStream;
  assert.equal(hasExactLiveMicrophone(exact, "mic-exact"), true);
  assert.equal(hasExactLiveMicrophone(foreign, "mic-exact"), false);
  assert.equal(hasExactLiveMicrophone(ended, "mic-exact"), false);
  assert.throws(() => exactMicrophoneConstraints("  "));
}

function assertCaptureOwnership() {
  resetCaptureOwnershipForTest();
  assert.equal(claimCaptureOwnership("manual"), true);
  assert.equal(claimCaptureOwnership("lark"), false);
  assert.equal(currentCaptureOwner(), "manual");
  releaseCaptureOwnership("manual");
  assert.equal(claimCaptureOwnership("lark"), true);
  assert.equal(claimCaptureOwnership("manual"), false);
  releaseCaptureOwnership("lark");
  assert.equal(currentCaptureOwner(), null);
}

function assertGenerationAndStopOrdering() {
  const disabled = initialLarkCaptureState();
  assert.equal(larkCaptureTransition(disabled, { type: "START", capture }).phase, "disabled");
  const starting = larkCaptureTransition(armed(), { type: "START", capture });
  assert.equal(starting.phase, "starting");
  assert.equal(larkCaptureTransition(starting, { type: "START", capture }), starting);
  const recording = larkCaptureTransition(starting, {
    type: "RECORDER_STARTED",
    operation: starting.operation,
  });
  assert.equal(recording.phase, "recording");
  assert.equal(larkCaptureTransition(recording, { type: "INVALIDATED", event: {
    pluginId: "other-plugin", generation: capture.generation, reason: "deviceRemoved",
  } }), recording);

  const stopping = larkCaptureTransition(recording, { type: "STOP", operation: recording.operation, intent: "send" });
  assert.equal(stopping.phase, "stopping");
  const nativeAckOnly = larkCaptureTransition(stopping, { type: "RECORDER_STOPPED", operation: recording.operation });
  assert.equal(nativeAckOnly.phase, "stopping", "recorder stop alone does not finish track release");
  const released = larkCaptureTransition(nativeAckOnly, { type: "TRACKS_RELEASED", operation: recording.operation });
  assert.equal(released.phase, "stopping", "browser cleanup alone waits for the host acknowledgement");
  const transcribing = larkCaptureTransition(released, { type: "HOST_CLEANUP_ACK", operation: recording.operation });
  assert.equal(transcribing.phase, "transcribing");

  for (const reason of ["deviceRemoved", "accessLost"] as const) {
    const invalidated = larkCaptureTransition(recording, { type: "INVALIDATED", event: {
      pluginId: capture.pluginId,
      generation: capture.generation,
      reason,
    } });
    assert.equal(invalidated.phase, "stopping");
    assert.equal(invalidated.stopIntent, "discard");
    assert.equal(invalidated.invalidation, reason);
    const stopped = larkCaptureTransition(invalidated, {
      type: "RECORDER_STOPPED", operation: recording.operation,
    });
    assert.equal(stopped.phase, "stopping");
    assert.equal(larkCaptureTransition(stopped, {
      type: "TRACKS_RELEASED", operation: recording.operation,
    }).phase, "error", "host invalidation plus actual recorder and track release finishes as error");
    assert.equal(larkCaptureTransition(invalidated, {
      type: "INVALIDATED", event: { ...capture, reason: "disabled" },
    }), invalidated);
  }

  const lost = larkCaptureTransition(transcribing, { type: "INVALIDATED", event: {
    pluginId: capture.pluginId, generation: capture.generation, reason: "accessLost",
  } });
  assert.equal(lost.phase, "error");
  assert.equal(larkCaptureTransition(lost, {
    type: "COMMIT_SUCCEEDED", operation: released.operation,
  }), lost, "late commit callback is inert after invalidation");
}

assertExactDevices();
assertCaptureOwnership();
assertGenerationAndStopOrdering();
console.log("larkCaptureModel: 3 fixtures passed");
