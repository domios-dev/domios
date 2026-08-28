import type { NormalizedChatMessage as ChatMessage } from "@codeterm/chat-core";
import type { TabStatus } from "../../lib/types";
import type { ChatMsgEvent } from "./chatSessionAuthority";

export const TEST_CHAT_REPLY_EVENT = "codeterm:test-chat-reply";
export const TEST_TAB_STATUS_EVENT = "codeterm:test-tab-status";
const TEST_CHAT_BASE_TIME_MS = 1_700_000_000_000;

type FrontendTestEnv = {
  DEV?: boolean;
  VITE_CODETERM_TEST?: string;
};

export interface TestChatReplyDetail {
  tabId: string;
  content?: string;
  id?: string;
  timestamp?: string;
  type?: ChatMessage["type"];
  provider?: string;
  messages?: ChatMessage[];
  sourceSessionId?: string | null;
  sessionBindingNonce?: string | null;
}

export interface TestTabStatusDetail {
  tabId: string;
  status: TabStatus;
}

export interface TestChatHandle {
  reply: (tabId: string, content: string, options?: Omit<TestChatReplyDetail, "tabId" | "content" | "messages">) => void;
  pushMessages: (tabId: string, messages: ChatMessage[]) => void;
  status: (tabId: string, status: TabStatus) => void;
  resetClock: () => void;
}

declare global {
  interface Window {
    __codetermTestChat?: TestChatHandle;
  }
}

export type ChatEventCommit = (payload: ChatMsgEvent) => void;
export type TabStatusEventCommit = (status: TabStatus) => void;

let testChatSequence = 0;
const testChatListenerCounts = new Map<string, number>();
const pendingTestChatReplies: TestChatReplyDetail[] = [];

function importEnv(): FrontendTestEnv | undefined {
  return (import.meta as { env?: FrontendTestEnv }).env;
}

function isLocalDevOrigin(): boolean {
  if (typeof window === "undefined") return false;
  const { hostname, port, protocol } = window.location;
  if (protocol !== "http:") return false;
  if (port === "7685") return false;
  return hostname === "127.0.0.1" || hostname === "localhost";
}

export function isCodeTermFrontendTestMode(env: FrontendTestEnv | undefined = importEnv()): boolean {
  const authoritativeDevRuntime = env?.DEV === true || isLocalDevOrigin();
  if (!authoritativeDevRuntime) return false;
  if (env?.VITE_CODETERM_TEST === "1") return true;
  if (typeof process !== "undefined" && process.env.VITE_CODETERM_TEST === "1") return true;
  // Runtime fallback: vite env does not survive detached dev starts (build
  // gateway), so harnesses request test mode via the page URL instead.
  if (typeof window !== "undefined" && window.location?.search) {
    return new URLSearchParams(window.location.search).has("e2e");
  }
  return false;
}

export function resetTestChatClockForCodeTerm(): void {
  testChatSequence = 0;
}

function nextTestChatTimestamp(): string {
  const value = TEST_CHAT_BASE_TIME_MS + testChatSequence;
  testChatSequence += 1;
  return String(value);
}

export function testChatReplyPayload(detail: TestChatReplyDetail): ChatMsgEvent {
  if (detail.messages) {
    return {
      tab_id: detail.tabId,
      messages: detail.messages,
      source_session_id: detail.sourceSessionId ?? null,
      session_binding_nonce: detail.sessionBindingNonce ?? null,
    };
  }
  return {
    tab_id: detail.tabId,
    messages: [
      {
        id: detail.id ?? `test-assistant-${detail.tabId}-${testChatSequence}`,
        type: detail.type ?? "assistant",
        content: detail.content ?? "",
        timestamp: detail.timestamp ?? nextTestChatTimestamp(),
        provider: detail.provider ?? "test",
      },
    ],
    source_session_id: detail.sourceSessionId ?? null,
    session_binding_nonce: detail.sessionBindingNonce ?? null,
  };
}

export function installCodeTermTestChatHandle(testMode = isCodeTermFrontendTestMode()): void {
  if (!testMode || typeof window === "undefined" || window.__codetermTestChat) return;
  window.__codetermTestChat = {
    reply(tabId, content, options = {}) {
      dispatchOrQueueTestChatReply({ ...options, tabId, content });
    },
    pushMessages(tabId, messages) {
      dispatchOrQueueTestChatReply({ tabId, messages });
    },
    status(tabId, status) {
      window.dispatchEvent(new CustomEvent(TEST_TAB_STATUS_EVENT, {
        detail: { tabId, status } satisfies TestTabStatusDetail,
      }));
    },
    resetClock: resetTestChatClockForCodeTerm,
  };
}

function dispatchOrQueueTestChatReply(detail: TestChatReplyDetail): void {
  if (testChatListenerCounts.get(detail.tabId)) {
    dispatchTestChatReply(detail);
    return;
  }
  pendingTestChatReplies.push(detail);
  dispatchTestChatReply(detail);
}

function dispatchTestChatReply(detail: TestChatReplyDetail): void {
  window.dispatchEvent(new CustomEvent(TEST_CHAT_REPLY_EVENT, { detail }));
}

function isTestTabStatus(status: unknown): status is TabStatus {
  return status === "Working" || status === "Waiting" || status === "Idle" || status === "Dead";
}

function drainPendingTestChatReplies(tabId: string, commit: ChatEventCommit): void {
  for (let i = 0; i < pendingTestChatReplies.length;) {
    const detail = pendingTestChatReplies[i];
    if (!detail || detail.tabId !== tabId) {
      i += 1;
      continue;
    }
    pendingTestChatReplies.splice(i, 1);
    commit(testChatReplyPayload(detail));
  }
}

export function listenForTestChatReplies(
  tabId: string,
  commit: ChatEventCommit,
): (() => void) | null {
  if (!isCodeTermFrontendTestMode() || typeof window === "undefined") return null;
  installCodeTermTestChatHandle();
  const handler = (event: Event) => {
    const detail = (event as CustomEvent<TestChatReplyDetail>).detail;
    if (!detail || detail.tabId !== tabId) return;
    commit(testChatReplyPayload(detail));
  };
  testChatListenerCounts.set(tabId, (testChatListenerCounts.get(tabId) ?? 0) + 1);
  window.addEventListener(TEST_CHAT_REPLY_EVENT, handler);
  drainPendingTestChatReplies(tabId, commit);
  return () => {
    const nextCount = (testChatListenerCounts.get(tabId) ?? 1) - 1;
    if (nextCount <= 0) testChatListenerCounts.delete(tabId);
    else testChatListenerCounts.set(tabId, nextCount);
    window.removeEventListener(TEST_CHAT_REPLY_EVENT, handler);
  };
}

export function listenForTestChatGrowth(
  tabId: string,
  commit: ChatEventCommit,
): (() => void) | null {
  if (!isCodeTermFrontendTestMode() || typeof window === "undefined") return null;
  installCodeTermTestChatHandle();
  const handler = (event: Event) => {
    const detail = (event as CustomEvent<TestChatReplyDetail>).detail;
    if (!detail || detail.tabId !== tabId) return;
    commit(testChatReplyPayload(detail));
  };
  window.addEventListener(TEST_CHAT_REPLY_EVENT, handler);
  return () => window.removeEventListener(TEST_CHAT_REPLY_EVENT, handler);
}

export function listenForTestTabStatus(
  tabId: string,
  commit: TabStatusEventCommit,
): (() => void) | null {
  if (!isCodeTermFrontendTestMode() || typeof window === "undefined") return null;
  installCodeTermTestChatHandle();
  const handler = (event: Event) => {
    const detail = (event as CustomEvent<TestTabStatusDetail>).detail;
    if (!detail || detail.tabId !== tabId || !isTestTabStatus(detail.status)) return;
    commit(detail.status);
  };
  window.addEventListener(TEST_TAB_STATUS_EVENT, handler);
  return () => window.removeEventListener(TEST_TAB_STATUS_EVENT, handler);
}
