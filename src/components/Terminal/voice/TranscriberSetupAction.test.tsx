import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { getByRole } from "../../../test/semanticRoles";

const bridge = vi.hoisted(() => ({ invoke: vi.fn() }));
const general = vi.hoisted(() => ({
  workspaces: [{ id: "general-workspace", protected: true, tabs: ["general-tab"] }],
  tabs: { "general-tab": { id: "general-tab", protected: true } },
}));
vi.mock("../../../lib/bridge", async actual => ({ ...await actual<object>(), invoke: bridge.invoke }));
vi.mock("../../../store/multiplexer/selectorStore", async actual => ({
  ...await actual<object>(), useMuxStateStore: () => ({ getState: () => general }),
}));
vi.mock("./useVoiceRecorder", () => ({
  useVoiceRecorder: () => ({
    recording: false, preview: null, livePeaks: [],
    start: async () => true, stop: async () => ["/voice.wav"],
  }),
}));

import { NeighborComposer } from "../../Neighbors/NeighborComposer";
import { PointAndAsk } from "../../Surface/PointAndAsk";
import { CartographerMapDialog } from "../../Canvas/CartographerMapDialog";
import { CARTOGRAPHER_TEST_PROVIDER } from "../../Canvas/cartographerMapFixtures.testdata";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let root: Root | undefined;
afterEach(async () => {
  await act(async () => root?.unmount());
  root = undefined;
  document.body.replaceChildren();
  vi.restoreAllMocks();
  bridge.invoke.mockReset();
});

async function mount(element: ReactNode) {
  const host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () => root!.render(element));
}

async function recordVoice() {
  const mic = document.querySelector<HTMLButtonElement>('button[title="Hold to record voice note"]');
  expect(mic).not.toBeNull();
  if (!mic) throw new Error("Voice recording control is unavailable");
  mic.setPointerCapture = () => {};
  mic.hasPointerCapture = () => false;
  await act(async () => mic.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, pointerId: 1, clientX: 100, clientY: 100 })));
  await act(async () => window.dispatchEvent(new PointerEvent("pointerup", { bubbles: true, pointerId: 1, clientX: 100, clientY: 100 })));
}

const setupAction = () => Array.from(document.querySelectorAll("button")).find(button => button.textContent === "Set up transcriber") ?? null;

describe("voice composer transcription recovery", () => {
  const surfaces = [
    { name: "Neighbor", render: (onError: (message: string) => void) => <NeighborComposer name="Ada" placeholder="Message" disabled={false} sending={false} onSend={async () => true} onError={onError} /> },
    { name: "Point and ask", render: (onError: (message: string) => void) => <PointAndAsk owner="Ada" subject={{ kind: "area", context: "Selected area", image: null, onRemoveImage() {} }} position={{ left: 0, top: 0 }} onSend={() => {}} onCancel={() => {}} onError={onError} /> },
    { name: "Cartographer", render: () => <CartographerMapDialog mode="create" providers={[CARTOGRAPHER_TEST_PROVIDER]} onStart={async () => {}} onCancel={() => {}} /> },
  ];
  for (const surface of surfaces) {
    it(`${surface.name} retains the transcription reason and sends setup to General`, async () => {
      const onError = vi.fn();
      let rejectTranscription!: (error: Error) => void;
      bridge.invoke.mockImplementation(async (command: string) => {
        if (command === "transcribe_voice_attachment") return new Promise((_resolve, reject) => { rejectTranscription = reject; });
      });
      await mount(surface.render(onError));
      expect(setupAction()).toBeNull();
      await recordVoice();
      expect(bridge.invoke).toHaveBeenCalledWith("transcribe_voice_attachment", { path: "/voice.wav" });
      if (surface.name === "Cartographer") expect(document.body.textContent).toContain("Transcribing description…");
      await act(async () => rejectTranscription(new Error("opaque transcription failure")));
      expect(document.querySelector('[role="status"][aria-label="opaque transcription failure"]')?.textContent).toContain("opaque transcription failure");
      expect(onError).not.toHaveBeenCalled();
      const focus = vi.fn();
      window.addEventListener("codeterm:focus-tab", focus);
      try {
        await act(async () => getByRole(document.body, "button", "Set up transcriber").click());
        expect(bridge.invoke).toHaveBeenCalledWith("chat_send_text", { tabId: "general-tab", text: expect.any(String) });
        expect(focus).toHaveBeenCalledTimes(1);
      } finally { window.removeEventListener("codeterm:focus-tab", focus); }
      expect(document.querySelector('[role="status"][aria-label="opaque transcription failure"]')).toBeTruthy();
      await recordVoice();
      expect(setupAction()).toBeNull();
      await act(async () => rejectTranscription(new Error("second transcription failure")));
      expect(document.querySelector('[role="status"][aria-label="second transcription failure"]')).toBeTruthy();
    });
  }
});
