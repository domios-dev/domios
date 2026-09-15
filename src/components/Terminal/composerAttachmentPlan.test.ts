import assert from "node:assert/strict";
import { planComposerAttachments } from "./composerAttachmentPlan";
import {
  AttachmentTransferError,
  composeAttachmentDraft,
  type AttachmentHandoffDeps,
  type MeshAttachmentTransferRequest,
} from "./meshAttachmentHandoff";

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

const sourceImage = "C:\\Users\\blackstar\\.codeterm\\uploads\\5e6f7a8b-pasted-image-1.png";
const sourceVoice = "C:\\Users\\blackstar\\.codeterm\\uploads\\1a2b3c4d-voice-note-1.webm";
const peerImage = "/Users/air/.codeterm/uploads/0a1b2c3d-5e6f7a8b-pasted-image-1.png";
const peerVoice = "/Users/air/.codeterm/uploads/9f8e7d6c-1a2b3c4d-voice-note-1.webm";

type Event = { step: "transfer"; request: MeshAttachmentTransferRequest } | { step: "transcribe-local"; path: string };

function consumer(relayPeerId: string | null, overrides: Partial<AttachmentHandoffDeps> = {}) {
  const events: Event[] = [];
  const deps: AttachmentHandoffDeps = {
    relayPeerId: () => relayPeerId,
    transfer: async (request) => {
      events.push({ step: "transfer", request });
      return request.kind === "voice"
        ? { path: peerVoice, kind: "voice", size: 9000, reference: `[voice ${peerVoice}: peer transcript]` }
        : { path: peerImage, kind: "file", size: 512, reference: null };
    },
    transcribeLocal: async (path) => {
      events.push({ step: "transcribe-local", path });
      return `[voice ${path}: origin transcript]`;
    },
    quote: (path) => (/\s/.test(path) ? `'${path}'` : path),
    ...overrides,
  };
  return { events, deps };
}

async function main() {
  {
    const { events, deps } = consumer(null);
    const draft = await composeAttachmentDraft("tab-7", "look", [sourceImage, sourceVoice], [sourceVoice], deps);
    assert.deepEqual(draft, {
      composed: `look ${sourceImage} [voice ${sourceVoice}: origin transcript]`,
      transcribeFailed: false,
    });
    assert.deepEqual(events, [{ step: "transcribe-local", path: sourceVoice }], "local sends never transfer");
  }

  {
    const { events, deps } = consumer(null);
    const draft = await composeAttachmentDraft("mesh:air/tab-7", "", [sourceVoice], [sourceVoice], deps);
    assert.deepEqual(events.map((event) => event.step), ["transfer"], "remote voice transcribes on the peer only");
    assert.deepEqual(draft, { composed: `[voice ${peerVoice}: peer transcript]`, transcribeFailed: false });
  }

  {
    const { events, deps } = consumer(null);
    const draft = await composeAttachmentDraft("mesh:air/tab-7", "look", [sourceImage], [], deps);
    assert.deepEqual(events, [{
      step: "transfer",
      request: { ordinaryTabId: "mesh:air/tab-7", peerId: "air", path: sourceImage, kind: "file" },
    }]);
    assert.equal(draft.composed, `look ${peerImage}`);
    assert.ok(!draft.composed.includes("blackstar"), "no source-machine path in the outgoing marker");
  }

  {
    const { events, deps } = consumer("air");
    const draft = await composeAttachmentDraft("tab-7", "", [sourceImage, sourceVoice], [sourceVoice], deps);
    assert.deepEqual(
      events.map((event) => (event.step === "transfer" ? [event.request.ordinaryTabId, event.request.kind] : event.step)),
      [["mesh:air/tab-7", "file"], ["mesh:air/tab-7", "voice"]],
      "a relayed web mirror hands bytes to the relay peer that owns the tab",
    );
    assert.equal(draft.composed, `${peerImage} [voice ${peerVoice}: peer transcript]`);
  }

  {
    const { events, deps } = consumer(null, { transfer: async () => { throw new Error("peer offline"); } });
    await assert.rejects(
      composeAttachmentDraft("mesh:air/tab-7", "look", [sourceImage, sourceVoice], [sourceVoice], deps),
      AttachmentTransferError,
    );
    assert.ok(events.every((event) => event.step !== "transcribe-local"));
  }

  {
    const { deps } = consumer(null);
    await assert.rejects(
      composeAttachmentDraft("mesh:air/tab-7", `see ${sourceImage}`, [sourceImage], [], deps),
      AttachmentTransferError,
      "typed text that still names a source attachment is refused before delivery",
    );
  }

  console.log("composer attachment source classification and host handoff: passed");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
