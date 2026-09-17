import assert from "node:assert/strict";
import {
  AttachmentTransferError,
  assertNoSourceAttachmentPaths,
  attachmentHostTarget,
  resolveAttachmentParts,
  type AttachmentHandoffDeps,
  type MeshAttachmentTransferRequest,
} from "./meshAttachmentHandoff";

const sourceVoice = "C:\\Users\\blackstar\\.codeterm\\uploads\\1a2b3c4d-voice-note-1.webm";
const sourceImage = "C:\\Users\\blackstar\\AppData\\Local\\Temp\\codeterm-chat\\5e6f7a8b-pasted image.png";
const targetVoice = "/Users/air/.codeterm/uploads/9f8e7d6c-1a2b3c4d-voice-note-1.webm";
const targetImage = "/Users/air/.codeterm/uploads/0a1b2c3d-5e6f7a8b-pasted_image.png";
const remoteTab = "mesh:air-peer/tab-7";
const remote = attachmentHostTarget(remoteTab, null);
const local = attachmentHostTarget("tab-7", null);
const plans = [
  { path: sourceVoice, transcribe: true },
  { path: sourceImage, transcribe: false },
];

const quote = (path: string) => (/\s/.test(path) ? `'${path}'` : path);

function harness(overrides: Partial<AttachmentHandoffDeps> = {}) {
  const transfers: MeshAttachmentTransferRequest[] = [];
  const transcribed: string[] = [];
  const deps: AttachmentHandoffDeps = {
    relayPeerId: () => null,
    transfer: async (request) => {
      transfers.push(request);
      const path = request.kind === "voice" ? targetVoice : targetImage;
      return {
        path,
        kind: request.kind,
        size: 42,
        reference: request.kind === "voice" ? `voice-marker(${path})` : null,
      };
    },
    transcribeLocal: async (path) => {
      transcribed.push(path);
      return `local-voice-marker(${path})`;
    },
    quote,
    ...overrides,
  };
  return { transfers, transcribed, deps };
}

async function main() {
  assert.deepEqual(local, { kind: "local" });
  assert.deepEqual(attachmentHostTarget("tab-7", "air-peer"), {
    kind: "remote",
    ordinaryTabId: remoteTab,
    peerId: "air-peer",
    remoteTabId: "tab-7",
  });
  assert.deepEqual(attachmentHostTarget(remoteTab, "other-peer"), remote, "a composite id names its own host");
  assert.equal(attachmentHostTarget("bad/tab", "air-peer").kind, "invalid-remote");

  {
    const { transfers, transcribed, deps } = harness();
    const parts = await resolveAttachmentParts(local, plans, deps);
    assert.equal(transfers.length, 0, "same-machine sends never transfer");
    assert.deepEqual(transcribed, [sourceVoice]);
    assert.deepEqual(parts, {
      parts: [`local-voice-marker(${sourceVoice})`, quote(sourceImage)],
      transcribeFailed: false,
    });
  }

  {
    const { deps } = harness({ transcribeLocal: async () => { throw new Error("no engine"); } });
    const parts = await resolveAttachmentParts(local, plans, deps);
    assert.equal(parts.transcribeFailed, true);
    assert.deepEqual(parts.parts, [quote(sourceVoice), quote(sourceImage)]);
  }

  {
    const { transfers, transcribed, deps } = harness();
    const parts = await resolveAttachmentParts(remote, plans, deps);
    assert.deepEqual(
      transfers.map(({ ordinaryTabId, peerId, path, kind }) => ({ ordinaryTabId, peerId, path, kind })),
      [
        { ordinaryTabId: remoteTab, peerId: "air-peer", path: sourceVoice, kind: "voice" },
        { ordinaryTabId: remoteTab, peerId: "air-peer", path: sourceImage, kind: "file" },
      ],
    );
    assert.equal(transcribed.length, 0, "voice is transcribed on the target host, not the origin");
    assert.deepEqual(parts.parts, [`voice-marker(${targetVoice})`, targetImage]);
    const composed = parts.parts.join(" ");
    assert.ok(!composed.includes(sourceVoice) && !composed.includes(sourceImage));
  }

  {
    const { deps } = harness({ transfer: async () => { throw new Error("peer_unroutable"); } });
    await assert.rejects(resolveAttachmentParts(remote, plans, deps), (error: unknown) => {
      assert.ok(error instanceof AttachmentTransferError);
      assert.ok(error.message.includes("peer_unroutable"));
      assert.ok(!error.message.includes("C:\\Users\\blackstar"), "the source path stays private");
      return true;
    });
  }

  {
    const oversized = "attachment of 26214401 bytes exceeds the 26214400-byte mesh limit";
    const { transcribed, deps } = harness({ transfer: async () => { throw new Error(oversized); } });
    await assert.rejects(resolveAttachmentParts(remote, [plans[0]], deps), (error: unknown) => {
      assert.ok(error instanceof AttachmentTransferError);
      assert.ok(error.message.includes("exceeds"));
      return true;
    });
    assert.equal(transcribed.length, 0, "an oversized note never falls back to origin transcription");
  }

  {
    const { deps } = harness({
      transfer: async (request) => ({ path: request.path, kind: request.kind, size: 1, reference: null }),
    });
    await assert.rejects(resolveAttachmentParts(remote, plans, deps), AttachmentTransferError);
  }

  {
    const { deps } = harness({
      transfer: async (request) => ({
        path: targetVoice,
        kind: request.kind,
        size: 1,
        reference: `voice-marker(${sourceVoice})`,
      }),
    });
    await assert.rejects(resolveAttachmentParts(remote, [plans[0]], deps), AttachmentTransferError);
  }

  await assert.rejects(
    resolveAttachmentParts(attachmentHostTarget("mesh:broken", null), plans, harness().deps),
    AttachmentTransferError,
  );

  assert.throws(
    () => assertNoSourceAttachmentPaths(remote, `look ${sourceImage}`, [sourceImage], quote),
    AttachmentTransferError,
  );
  assert.throws(
    () => assertNoSourceAttachmentPaths(remote, `look ${quote(sourceImage)}`, [sourceImage], quote),
    AttachmentTransferError,
  );
  assert.doesNotThrow(() => assertNoSourceAttachmentPaths(local, `look ${sourceImage}`, [sourceImage], quote));
  assert.doesNotThrow(() => assertNoSourceAttachmentPaths(remote, `look ${targetImage}`, [sourceImage], quote));
}

main().then(
  () => console.log("mesh attachment handoff: passed"),
  (error) => {
    console.error(error);
    process.exit(1);
  },
);
