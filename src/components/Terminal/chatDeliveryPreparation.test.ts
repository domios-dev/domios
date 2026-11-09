import "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { test } from "node:test";
import { markVoice } from "@codeterm/chat-core";
import { __setInvokeForTest } from "../../lib/bridge";
import { clearRelayTarget, setRelayTarget } from "../../lib/webApi";
import { prepareChatDelivery } from "./chatDeliveryPreparation";
import { addChatSend, beginChatSendRetry, forgetChatSendsInMemory, readChatSends, setChatSendDraft,
  __setChatSendRepositoryForTest, ensureChatSendTabHydrated } from "./chatSendStore";
import { deliveryState } from "./chatDeliveryModel";
import { MemoryChatSendRepository } from "./chatSendRepository.testSupport";

Object.defineProperty(globalThis, "localStorage", { value: window.localStorage, configurable: true });
__setChatSendRepositoryForTest(new MemoryChatSendRepository());

const path = "C:\\Users\\source-user\\voice-note.webm";
const peerPath = "/Users/peer-user/voice-note.webm";
const expansion = () => ({ kind: "none" as const });

function draft(tab: string) {
  addChatSend(tab, { id: "voice", correlationId: "voice", content: "pending", sentAtMs: 10 });
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
    await ensureChatSendTabHydrated(tab);
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


test("asynchronous requested task expansion completes before prepared content replaces the draft", async () => {
  const tab = "task-window-preparation";
  addChatSend(tab,{id:"tasks",content:"pending",sentAtMs:20});
  setChatSendDraft(tab,"tasks",{text:"<ds-tasks />",attachments:[],recordedVoiceAttachments:[],capsules:{},reply:null});
  const original=readChatSends(tab)[0];
  let finish!: (value: {kind:"expanded";text:string})=>void;
  const window=new Promise<{kind:"expanded";text:string}>((resolve)=>{finish=resolve;});
  const preparing=prepareChatDelivery(tab,original,()=>window);
  assert.equal(readChatSends(tab)[0].content,"pending");
  assert.ok(readChatSends(tab)[0].draft);
  finish({kind:"expanded",text:"Requested task window"});
  assert.equal(await preparing,"Requested task window");
  assert.equal(readChatSends(tab)[0].content,"Requested task window");
  assert.equal(readChatSends(tab)[0].draft,undefined);
  assert.equal(readChatSends(tab).length,1);
});
