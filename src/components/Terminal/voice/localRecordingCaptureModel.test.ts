import assert from "node:assert/strict";
import {
  claimCaptureOwnership,
  currentCaptureOwner,
  exactMicrophoneConstraints,
  hasExactLiveMicrophone,
  initialLocalRecordingCaptureState,
  localRecordingCaptureTransition,
  releaseCaptureOwnership,
  resetCaptureOwnershipForTest,
  type LocalRecordingCaptureLease,
} from "./localRecordingCaptureModel";

const status = {
  pluginId: "lark",
  granted: true,
  enabled: true,
  available: true,
  generation: 12,
};

const capture: LocalRecordingCaptureLease = {
  lease: "lease-1",
  pluginId: "lark",
  tabId: "tab-A",
  microphoneId: "mic-exact",
  generation: 12,
};

function armed() {
  return localRecordingCaptureTransition(initialLocalRecordingCaptureState(), { type: "STATUS", status });
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
  assert.equal(claimCaptureOwnership("hardware"), false);
  assert.equal(currentCaptureOwner(), "manual");
  releaseCaptureOwnership("manual");
  assert.equal(claimCaptureOwnership("hardware"), true);
  assert.equal(claimCaptureOwnership("manual"), false);
  releaseCaptureOwnership("hardware");
  assert.equal(currentCaptureOwner(), null);
}

function assertGenerationAndStopOrdering() {
  const disabled = initialLocalRecordingCaptureState();
  assert.equal(localRecordingCaptureTransition(disabled, { type: "START", capture }).phase, "disabled");
  const starting = localRecordingCaptureTransition(armed(), { type: "START", capture });
  assert.equal(starting.phase, "starting");
  assert.equal(localRecordingCaptureTransition(starting, { type: "START", capture }), starting);
  const recording = localRecordingCaptureTransition(starting, {
    type: "RECORDER_STARTED",
    operation: starting.operation,
  });
  assert.equal(recording.phase, "recording");
  assert.equal(localRecordingCaptureTransition(recording, { type: "INVALIDATED", event: {
    pluginId: "other-plugin", generation: capture.generation, reason: "deviceRemoved",
  } }), recording);

  const stopping = localRecordingCaptureTransition(recording, { type: "STOP", operation: recording.operation, intent: "send" });
  assert.equal(stopping.phase, "stopping");
  const nativeAckOnly = localRecordingCaptureTransition(stopping, { type: "RECORDER_STOPPED", operation: recording.operation });
  assert.equal(nativeAckOnly.phase, "stopping", "recorder stop alone does not finish track release");
  const released = localRecordingCaptureTransition(nativeAckOnly, { type: "TRACKS_RELEASED", operation: recording.operation });
  assert.equal(released.phase, "stopping", "browser cleanup waits for explicit stopped-lease acknowledgement");
  const transcribing = localRecordingCaptureTransition(released, { type: "CAPTURE_STOP_ACK", operation: recording.operation });
  assert.equal(transcribing.phase, "transcribing");

  for (const reason of ["deviceRemoved", "accessLost"] as const) {
    const invalidated = localRecordingCaptureTransition(recording, { type: "INVALIDATED", event: {
      pluginId: capture.pluginId,
      generation: capture.generation,
      reason,
    } });
    assert.equal(invalidated.phase, "stopping");
    assert.equal(invalidated.stopIntent, "discard");
    assert.equal(invalidated.invalidation, reason);
    const stopped = localRecordingCaptureTransition(invalidated, {
      type: "RECORDER_STOPPED", operation: recording.operation,
    });
    assert.equal(stopped.phase, "stopping");
    assert.equal(localRecordingCaptureTransition(stopped, {
      type: "TRACKS_RELEASED", operation: recording.operation,
    }).phase, "error", "host invalidation plus actual recorder and track release finishes as error");
    assert.equal(localRecordingCaptureTransition(invalidated, {
      type: "INVALIDATED", event: { ...capture, reason: "disabled" },
    }), invalidated);
  }

  const lost = localRecordingCaptureTransition(transcribing, { type: "INVALIDATED", event: {
    pluginId: capture.pluginId, generation: capture.generation, reason: "accessLost",
  } });
  assert.equal(lost.phase, "error");
  assert.equal(localRecordingCaptureTransition(lost, {
    type: "COMMIT_SUCCEEDED", operation: released.operation,
  }), lost, "late commit callback is inert after invalidation");

  const newerCapture = { ...capture, lease: "lease-new", generation: 13 };
  const newerArmed = localRecordingCaptureTransition(armed(), {
    type: "STATUS", status: { ...status, generation: 13 },
  });
  const newerStarting = localRecordingCaptureTransition(newerArmed, { type: "START", capture: newerCapture });
  const staleInvalidation = localRecordingCaptureTransition(newerStarting, { type: "INVALIDATED", event: {
    pluginId: capture.pluginId, generation: capture.generation, reason: "deviceRemoved",
  } });
  assert.equal(staleInvalidation, newerStarting, "the old generation cannot invalidate a replacement capture");

  const cancelled = localRecordingCaptureTransition(recording, {
    type: "CANCEL", operation: recording.operation, message: "Voice recording cancelled.",
  });
  assert.equal(cancelled.phase, "stopping");
  assert.equal(localRecordingCaptureTransition(cancelled, {
    type: "RECORDER_STOPPED", operation: recording.operation,
  }).phase, "stopping", "cancel command completion is not recorder teardown acknowledgement");
  assert.equal(localRecordingCaptureTransition(localRecordingCaptureTransition(cancelled, {
    type: "RECORDER_STOPPED", operation: recording.operation,
  }), {
    type: "TRACKS_RELEASED", operation: recording.operation,
  }).phase, "error", "cancel settles only after actual recorder and track release");
}

assertExactDevices();
assertCaptureOwnership();
assertGenerationAndStopOrdering();
console.log("localRecordingCaptureModel: 3 fixtures passed");
