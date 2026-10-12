import assert from "node:assert/strict";
import { offeredRecordingSelection } from "./localRecordingSelection";

const inputs = [{ inputId: "receiver", collectionId: "collection" }];
const microphones = [{ deviceId: "microphone", label: "Opaque label" },
  { deviceId: "default", label: "System default" }];
const selection = { ...inputs[0], microphoneId: "microphone" };
assert.deepEqual(offeredRecordingSelection(selection, inputs, microphones), selection);
for (const candidate of [null, {}, { ...selection, inputId: "foreign" },
  { ...selection, collectionId: "foreign" }, { ...selection, microphoneId: "foreign" },
  { ...selection, microphoneId: "default" }, { ...selection, microphoneId: 1 }]) {
  assert.equal(offeredRecordingSelection(candidate, inputs, microphones), null);
}
console.log("localRecordingSelection: offered-device boundary fixtures passed");
