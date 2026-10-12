import assert from "node:assert/strict";
import {
  claimCaptureOwnership,
  currentCaptureOwner,
  releaseCaptureOwnership,
  resetCaptureOwnershipForTest,
} from "./localRecordingCaptureModel";

function assertManualAndHardwareInterlock() {
  resetCaptureOwnershipForTest();
  assert.equal(claimCaptureOwnership("manual"), true, "manual start claims before asynchronous microphone permission");
  assert.equal(claimCaptureOwnership("hardware"), false, "hardware START cannot race a starting manual capture");
  assert.equal(currentCaptureOwner(), "manual");
  releaseCaptureOwnership("manual");
  assert.equal(claimCaptureOwnership("hardware"), true, "hardware may start after manual cancel or completion releases ownership");
  assert.equal(claimCaptureOwnership("manual"), false, "manual preview remains excluded while hardware owns the microphone");
  releaseCaptureOwnership("hardware");
  assert.equal(currentCaptureOwner(), null);
}

assertManualAndHardwareInterlock();
resetCaptureOwnershipForTest();
console.log("useVoiceRecorder: 1 manual capture interlock fixture passed");
