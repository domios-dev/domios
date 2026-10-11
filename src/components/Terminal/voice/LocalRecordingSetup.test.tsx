import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { Window } from "happy-dom";

async function main() {
  const testWindow = new Window();
  const descriptors = new Map<string, PropertyDescriptor | undefined>();
  for (const [key, value] of Object.entries({ window: testWindow, document: testWindow.document,
    navigator: testWindow.navigator, IS_REACT_ACT_ENVIRONMENT: true })) {
    descriptors.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { configurable: true, value });
  }
  const { LocalRecordingSetup } = await import("./LocalRecordingSetup");
  const bridge = await import("../../../lib/bridge");
  let inputs: Array<{ inputId: string; collectionId: string }> = [];
  let enumerations = 0;
  let captures = 0;
  let closed = 0;
  const selections: Record<string, unknown>[] = [];
  Object.defineProperty(testWindow.navigator, "mediaDevices", { configurable: true, value: {
    enumerateDevices: async () => {
      enumerations += 1;
      return [{ kind: "audioinput", deviceId: "mic-selected", label: "Selected microphone" },
        { kind: "videoinput", deviceId: "camera", label: "Camera" }];
    },
    getUserMedia: async () => { captures += 1; throw new Error("setup must not acquire audio"); },
  } });
  bridge.__setInvokeForTest(async <T,>(command: string, args: Record<string, unknown> = {}) => {
    if (command === "local_recording_input_devices") return inputs as T;
    if (command === "local_recording_select_devices") { selections.push(args); return undefined as T; }
    throw new Error(`unexpected command ${command}`);
  });
  const container = testWindow.document.createElement("div");
  testWindow.document.body.append(container);
  const root = createRoot(container as unknown as Element);
  try {
    await act(async () => root.render(<LocalRecordingSetup pluginId="absent" onClose={() => closed++} />));
    assert.equal(container.querySelector('[role="dialog"]'), null);
    assert.equal(enumerations, 0, "no receiver acquires no microphone inventory or permissions");
    inputs = [{ inputId: "exact-input", collectionId: "Col01" }];
    await act(async () => root.render(<LocalRecordingSetup pluginId="present" onClose={() => closed++} />));
    const selects = container.querySelectorAll("select");
    assert.equal(selects.length, 2);
    assert.equal(selects[0].value, "");
    assert.equal(selects[1].value, "");
    assert.equal(selects[1].options.length, 2, "only audio input is selectable");
    assert.equal(container.querySelector("button")!.disabled, true);
    await act(async () => {
      selects[0].value = "exact-input";
      selects[0].dispatchEvent(new testWindow.Event("change", { bubbles: true }));
      selects[1].value = "mic-selected";
      selects[1].dispatchEvent(new testWindow.Event("change", { bubbles: true }));
    });
    await act(async () => container.querySelector("button")!.click());
    assert.deepEqual(selections, [{ pluginId: "present", inputId: "exact-input", collectionId: "Col01", microphoneId: "mic-selected" }]);
    assert.equal(closed, 1);
    assert.equal(captures, 0, "explicit setup still records no audio");
    console.log("LocalRecordingSetup: 3 wiring fixtures passed");
  } finally {
    await act(async () => root.unmount());
    bridge.__setInvokeForTest(null);
    testWindow.happyDOM.abort();
    for (const [key, descriptor] of descriptors) {
      if (descriptor) Object.defineProperty(globalThis, key, descriptor);
      else Reflect.deleteProperty(globalThis, key);
    }
  }
}
main().catch((error) => { console.error(error); process.exitCode = 1; });
