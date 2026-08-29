import assert from "node:assert/strict";
import { planComposerAttachments } from "./composerAttachmentPlan";

const attachedVideo = "C:\\captures\\screen-recording.mp4";
const recordedVoice = "C:\\temp\\voice-note.webm";

const plan = planComposerAttachments(
  [attachedVideo, recordedVoice],
  [recordedVoice],
);

assert.deepEqual(plan, [
  { path: attachedVideo, transcribe: false },
  { path: recordedVoice, transcribe: true },
]);

console.log("composer attachment source classification: passed");
