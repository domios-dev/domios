import "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import { markVoice } from "@codeterm/chat-core";
import { __setInvokeForTest } from "../../lib/bridge";
import { clearRelayTarget, setRelayTarget } from "../../lib/webApi";
import { prepareChatDelivery } from "./chatDeliveryPreparation";
import { addChatSend, beginChatSendRetry, forgetChatSendsInMemory, readChatSends, setChatSendDraft } from "./chatSendStore";
import { deliveryState } from "./chatDeliveryModel";

Object.defineProperty(globalThis, "localStorage", { value: window.localStorage, configurable: true });

const path = "C:\\Users\\blackstar\\voice-note.webm";
const peerPath = "/Users/eight/voice-note.webm";
const expansion = () => ({ kind: "none" as const });

function draft(tab: string) {
  addChatSend(tab, { id: "voice", content: "pending", sentAtMs: 10 });
  setChatSendDraft(tab, "voice", {
    text: "", attachments: [path], recordedVoiceAttachments: [path], capsules: {}, reply: null,
  });
  return readChatSends(tab)[0];
}

test("remote voice preparation retains an explicit failure and retries the original draft on one bubble", async () => {
  const tab = "mesh:eight/voice-preparation";
  const original = draft(tab);
  const calls: string[] = [];
  let reference = markVoice(peerPath, null, "engine unavailable: model not installed");
  __setInvokeForTest(async <T>(command: string) => {
    calls.push(command);
    assert.equal(command, "mesh_transfer_attachment");
    return { path: peerPath, kind: "voice", size: 9000, reference } as T;
  });
  try {
    await assert.rejects(prepareChatDelivery(tab, original, expansion), /model not installed/);
    forgetChatSendsInMemory();
    const failed = readChatSends(tab)[0];
    assert.equal(deliveryState(failed), "failed");
    assert.match(failed.content, /model not installed/);
    assert.deepEqual(failed.draft, original.draft);
    assert.equal(failed.sentAtMs, 10);
    reference = markVoice(peerPath, "spoken text");
    const retry = beginChatSendRetry(tab, "voice")!;
    assert.equal(await prepareChatDelivery(tab, retry, expansion), reference);
    assert.equal(readChatSends(tab).length, 1);
    assert.equal(readChatSends(tab)[0].draft, undefined);
    assert.deepEqual(calls, ["mesh_transfer_attachment", "mesh_transfer_attachment"]);
  } finally {
    __setInvokeForTest(null);
  }
});

test("desktop remote-control host failures keep the underlying reason in the bubble", async () => {
  setRelayTarget("eight", "eight");
  const tab = "desktop-relayed-voice";
  const original = draft(tab);
  __setInvokeForTest(async () => { throw new Error("remote host connection refused"); });
  try {
    await assert.rejects(prepareChatDelivery(tab, original, expansion), /connection refused/);
    assert.match(readChatSends(tab)[0].content, /connection refused/);
    assert.equal(deliveryState(readChatSends(tab)[0]), "failed");
  } finally {
    clearRelayTarget();
    __setInvokeForTest(null);
  }
});
