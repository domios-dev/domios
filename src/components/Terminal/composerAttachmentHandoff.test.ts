import assert from "node:assert/strict";
import { Window } from "happy-dom";

const browser = new Window({ url: "http://localhost:7686/" });
for (const key of ["window", "document", "localStorage", "sessionStorage", "DOMException", "navigator"] as const) {
  Object.defineProperty(globalThis, key, {
    value: (browser as unknown as Record<string, unknown>)[key],
    configurable: true,
  });
}

const sourceImage = "C:\\Users\\blackstar\\.codeterm\\uploads\\5e6f7a8b-pasted-image-1.png";
const sourceVoice = "C:\\Users\\blackstar\\.codeterm\\uploads\\1a2b3c4d-voice-note-1.webm";
const peerImage = "/Users/air/.codeterm/uploads/0a1b2c3d-5e6f7a8b-pasted-image-1.png";
const peerVoice = "/Users/air/.codeterm/uploads/9f8e7d6c-1a2b3c4d-voice-note-1.webm";

type Call = { url: string; body: Record<string, unknown> | null };

async function main() {
  const { clearRelayTarget, IS_WEB, selfHost, setRelayTarget, setTokenFor } = await import("../../lib/webApi");
  const { composerAttachmentHandoff } = await import("./composerAttachmentHandoff");
  const { composeAttachmentDraft } = await import("./meshAttachmentHandoff");
  assert.equal(IS_WEB, true);
  setTokenFor(selfHost(), "test-token");

  const calls: Call[] = [];
  globalThis.fetch = async (input, init) => {
    const url = String(input);
    const body = typeof init?.body === "string" ? JSON.parse(init.body) as Record<string, unknown> : null;
    calls.push({ url, body });
    if (url.endsWith("/api/mesh/attachments/transfer")) {
      const voice = body?.kind === "voice";
      return new Response(JSON.stringify({
        path: voice ? peerVoice : peerImage,
        kind: body?.kind,
        size: 9000,
        reference: voice ? `[voice ${peerVoice}: peer transcript]` : null,
      }), { status: 200 });
    }
    if (url.endsWith("/api/transcribe-voice")) {
      return new Response(JSON.stringify({ line: `[voice ${String(body?.path)}: origin transcript]` }), { status: 200 });
    }
    return new Response("unexpected", { status: 500 });
  };

  setRelayTarget("air", "Air");
  assert.equal(composerAttachmentHandoff.relayPeerId(), "air");
  const relayed = await composeAttachmentDraft(
    "tab-7",
    "look",
    [sourceImage, sourceVoice],
    [sourceVoice],
    composerAttachmentHandoff,
  );
  assert.deepEqual(
    calls.map(({ url, body }) => [url, body]),
    [
      [`${selfHost()}/api/mesh/attachments/transfer`, { ordinary_tab_id: "mesh:air/tab-7", peer_id: "air", path: sourceImage, kind: "file" }],
      [`${selfHost()}/api/mesh/attachments/transfer`, { ordinary_tab_id: "mesh:air/tab-7", peer_id: "air", path: sourceVoice, kind: "voice" }],
    ],
    "the serving daemon that holds the bytes runs the handoff; nothing rides relay-to",
  );
  assert.equal(relayed.composed, `look ${peerImage} [voice ${peerVoice}: peer transcript]`);
  assert.ok(!relayed.composed.includes("blackstar"));

  calls.length = 0;
  clearRelayTarget();
  assert.equal(composerAttachmentHandoff.relayPeerId(), null);
  await composeAttachmentDraft("mesh:air/tab-7", "", [sourceImage], [], composerAttachmentHandoff);
  assert.deepEqual(calls.map(({ url }) => url), [`${selfHost()}/api/mesh/attachments/transfer`]);

  calls.length = 0;
  const local = await composeAttachmentDraft("tab-7", "", [sourceImage, sourceVoice], [sourceVoice], composerAttachmentHandoff);
  assert.deepEqual(calls.map(({ url }) => url), [`${selfHost()}/api/transcribe-voice`]);
  assert.equal(local.composed, `${sourceImage} [voice ${sourceVoice}: origin transcript]`);
}

main()
  .then(() => {
    console.log("composer attachment handoff web routing: passed");
    process.exit(0);
  })
  .catch((error) => {
    console.error(error);
    process.exit(1);
  });
