import assert from "node:assert/strict";
import {
  AttachmentTransferError,
  assertNoSourceAttachmentPaths,
  resolveAttachmentParts,
  type AttachmentHandoffDeps,
  type MeshAttachmentTransferRequest,
} from "./meshAttachmentHandoff";

const sourceVoice = "C:\\Users\\blackstar\\.codeterm\\uploads\\1a2b3c4d-voice-note-1.webm";
const sourceImage = "C:\\Users\\blackstar\\AppData\\Local\\Temp\\codeterm-chat\\5e6f7a8b-pasted image.png";
const targetVoice = "/Users/air/.codeterm/uploads/9f8e7d6c-1a2b3c4d-voice-note-1.webm";
const targetImage = "/Users/air/.codeterm/uploads/0a1b2c3d-5e6f7a8b-pasted_image.png";
const remoteTab = "mesh:air-peer/tab-7";
const plans = [
  { path: sourceVoice, transcribe: true },
  { path: sourceImage, transcribe: false },
];

const quote = (path: string) => (/\s/.test(path) ? `'${path}'` : path);

function harness(overrides: Partial<AttachmentHandoffDeps> = {}) {
  const transfers: MeshAttachmentTransferRequest[] = [];
  const transcribed: string[] = [];
  const deps: AttachmentHandoffDeps = {
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
  {
    const { transfers, transcribed, deps } = harness();
    const local = await resolveAttachmentParts("tab-7", plans, deps);
    assert.equal(transfers.length, 0, "same-machine sends never transfer");
    assert.deepEqual(transcribed, [sourceVoice]);
    assert.deepEqual(local, {
      parts: [`local-voice-marker(${sourceVoice})`, quote(sourceImage)],
      transcribeFailed: false,
    });
  }

  {
    const { deps } = harness({ transcribeLocal: async () => { throw new Error("no engine"); } });
    const local = await resolveAttachmentParts("tab-7", plans, deps);
    assert.equal(local.transcribeFailed, true);
    assert.deepEqual(local.parts, [quote(sourceVoice), quote(sourceImage)]);
  }

  {
    const { transfers, transcribed, deps } = harness();
    const remote = await resolveAttachmentParts(remoteTab, plans, deps);
    assert.deepEqual(
      transfers.map(({ ordinaryTabId, peerId, path, kind }) => ({ ordinaryTabId, peerId, path, kind })),
      [
        { ordinaryTabId: remoteTab, peerId: "air-peer", path: sourceVoice, kind: "voice" },
        { ordinaryTabId: remoteTab, peerId: "air-peer", path: sourceImage, kind: "file" },
      ],
    );
    assert.equal(transcribed.length, 0, "voice is transcribed on the target host, not the origin");
    assert.deepEqual(remote.parts, [`voice-marker(${targetVoice})`, targetImage]);
    assert.notEqual(remote.parts[0], remote.parts[1]);
    const composed = remote.parts.join(" ");
    assert.ok(!composed.includes(sourceVoice) && !composed.includes(sourceImage));
  }

  {
    const { deps } = harness({ transfer: async () => { throw new Error("peer_unroutable"); } });
    await assert.rejects(resolveAttachmentParts(remoteTab, plans, deps), (error: unknown) => {
      assert.ok(error instanceof AttachmentTransferError);
      assert.ok(error.message.includes("peer_unroutable"));
      assert.ok(!error.message.includes("C:\\Users\\blackstar"), "the source path stays private");
      return true;
    });
  }

  {
    const { deps } = harness({
      transfer: async (request) => ({ path: request.path, kind: request.kind, size: 1, reference: null }),
    });
    await assert.rejects(resolveAttachmentParts(remoteTab, plans, deps), AttachmentTransferError);
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
    await assert.rejects(resolveAttachmentParts(remoteTab, [plans[0]], deps), AttachmentTransferError);
  }

  await assert.rejects(resolveAttachmentParts("mesh:broken", plans, harness().deps), AttachmentTransferError);

  assert.throws(
    () => assertNoSourceAttachmentPaths(remoteTab, `look ${sourceImage}`, [sourceImage], quote),
    AttachmentTransferError,
  );
  assert.throws(
    () => assertNoSourceAttachmentPaths(remoteTab, `look ${quote(sourceImage)}`, [sourceImage], quote),
    AttachmentTransferError,
  );
  assert.doesNotThrow(() => assertNoSourceAttachmentPaths("tab-7", `look ${sourceImage}`, [sourceImage], quote));
  assert.doesNotThrow(() => assertNoSourceAttachmentPaths(remoteTab, `look ${targetImage}`, [sourceImage], quote));
}

main().then(
  () => console.log("mesh attachment handoff: passed"),
  (error) => {
    console.error(error);
    process.exit(1);
  },
);
