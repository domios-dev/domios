import { OutcomeUnknown, RetryableMuxError } from "../../lib/streamMux";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { __setInvokeForTest, __setListenForTest } from "../../lib/bridge";
import { useChatSession } from "./useChatSession";
import { clearTranscriptWindows } from "./transcriptStore";
import { chatBodyView } from "./chatBodyVisibility";
import { ChatSetupState } from "./ChatSetupState";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let host: HTMLDivElement;
let root: Root;
let starts: number;
let failSnapshot: boolean;
let failListener: boolean;
const authority = { source_session_id: "own-session", session_binding_nonce: "own-nonce" };

function Harness({ active = true }: { active?: boolean }) {
  const session = useChatSession({ tabId: "failed-attach", agentSessionId: "own-session",
    sessionBindingNonce: "own-nonce", active, retainAttachment: true });
  const view = chatBodyView({ snapshotReady: session.snapshotReady, hasTranscript: session.allMessages.length > 0,
    chatLoading: session.loading, attachmentError: session.attachmentState === "failed" ? session.error : null,
    unavailable: session.transcriptUnavailable, bootView: { kind: "none" } });
  return <><ChatSetupState tab={null} providerName="Fixture" view={view} onRetry={session.retry} />
    {view.kind === "none" && <div role="log">{session.allMessages.map(row => row.content).join("")}</div>}</>;
}

beforeEach(() => {
  clearTranscriptWindows();
  starts = 0;
  failSnapshot = true;
  failListener = false;
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  __setInvokeForTest((async (command: string, args: Record<string, unknown>) => {
    if (command !== "chat_start") return [];
    starts++;
    if (failSnapshot) throw new Error("request_busy");
    return { ...authority, subscription_id: args.subscriptionId, messages: [{ id: "row", type: "assistant", content: "Restored transcript" }],
      text: "", start_offset: 0, offset: 40, pending: false };
  }) as never);
  __setListenForTest(async event => {
    if (event === "tab-chat-msg-failed-attach" && failListener) throw new Error("listener refused");
    return () => {};
  });
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  __setInvokeForTest(null);
  __setListenForTest(null);
  clearTranscriptWindows();
  vi.useRealTimers();
});

it("reports an unbound refused-start snapshot once across time and parent renders", async () => {
  vi.useFakeTimers();
  let failures = 0;
  let latest: ReturnType<typeof useChatSession> | undefined;
  const error = "runtime_history_unavailable: Runtime start failed before a conversation was bound; start a new session";
  __setInvokeForTest((async (command: string) => {
    if (command !== "chat_start") return [];
    starts++;
    throw new Error(error);
  }) as never);
  function RefusedStart({ revision }: { revision: number }) {
    latest = useChatSession({ tabId: "refused-unbound", agentSessionId: null,
      sessionBindingNonce: "refused-nonce", active: true, allowUnboundAuthority: true,
      onError: () => { failures++; } });
    return <div data-revision={revision}>{latest.error}</div>;
  }
  await act(async () => root.render(<RefusedStart revision={0} />));
  expect(starts).toBe(1);
  expect(failures).toBe(1);
  expect(latest?.attachmentState).toBe("failed");
  expect(latest?.loading).toBe(false);
  expect(host.textContent).toBe(error);
  await act(async () => { await vi.advanceTimersByTimeAsync(30_000); });
  await act(async () => root.render(<RefusedStart revision={1} />));
  expect(starts).toBe(1);
  expect(failures).toBe(1);
  expect(vi.getTimerCount()).toBe(0);
});

it("shows a failed snapshot and Retry commits a fresh authoritative transcript", async () => {
  await act(async () => root.render(<Harness />));
  expect(host.querySelector('[aria-label="Chat connection failed"]')?.textContent).toContain("request_busy");
  expect(host.querySelector('[data-testid="chat-setup-state"]')).toBeNull();
  expect(starts).toBe(1);
  failSnapshot = false;
  await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
  expect(host.querySelector('[role="log"]')?.textContent).toBe("Restored transcript");
  expect(host.querySelector("button")).toBeNull();
  expect(starts).toBe(2);
});

it("keeps transport diagnostics collapsed behind a human connection summary", async () => {
  __setInvokeForTest((async () => { throw new Error('/api/tabs/opaque/chat-raw/start → HTTP 400: Provider refused: ProviderRefused: opaque-session'); }) as never);
  await act(async () => root.render(<Harness />));
  const banner=host.querySelector('[role="status"][aria-label="Chat connection failed"]')!;
  expect(banner).not.toBeNull();
  expect(banner.querySelector("details")?.open).toBe(false);
  expect(banner.querySelector("summary")?.textContent).toBe("Details");
  expect(banner.querySelector("pre")?.textContent).toContain("HTTP 400");
  expect(banner.querySelector("details")?.previousElementSibling?.textContent).toBe("The connection to the agent could not be completed.");
});

it("retries a failed snapshot on tab lifecycle activation", async () => {
  await act(async () => root.render(<Harness />));
  failSnapshot = false;
  await act(async () => root.render(<Harness active={false} />));
  await act(async () => root.render(<Harness />));
  expect(host.querySelector('[role="log"]')?.textContent).toBe("Restored transcript");
  expect(starts).toBe(2);
});

it("Retry reinstalls a rejected event listener before requesting the snapshot", async () => {
  failListener = true;
  failSnapshot = false;
  await act(async () => root.render(<Harness />));
  expect(host.textContent).toContain("listener refused");
  expect(starts).toBe(0);
  failListener = false;
  await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
  expect(host.querySelector('[role="log"]')?.textContent).toBe("Restored transcript");
  expect(starts).toBe(1);
});

it("recovers a superseded start without presenting a terminal attach failure", async () => {
  let latest: ReturnType<typeof useChatSession> | undefined;
  __setInvokeForTest((async (command: string, args: Record<string, unknown>) => {
    if (command !== "chat_start") return [];
    starts++;
    if (starts === 1) throw new DOMException("Attachment superseded", "AbortError");
    return { ...authority, subscription_id: args.subscriptionId, messages: [{ id: "row", type: "assistant", content: "Restored transcript" }], text: "", start_offset: 0, offset: 40, pending: false };
  }) as never);
  const failures: string[] = [];
  function Probe() {
    latest = useChatSession({ tabId: "superseded", agentSessionId: "own-session", sessionBindingNonce: "own-nonce", active: true, onError: error => failures.push(error) });
    return <div>{latest.allMessages.map(row => row.content).join("")}</div>;
  }
  await act(async () => root.render(<Probe />));
  expect(failures).toEqual([]);
  expect(latest?.attachmentState).toBe("attached");
  expect(host.textContent).toBe("Restored transcript");
  expect(starts).toBe(2);
});
it("bounds aborted attach recovery and resumes on a later activation", async () => {
  let resumed = false;
  let latest: ReturnType<typeof useChatSession> | undefined;
  __setInvokeForTest((async (command: string, args: Record<string, unknown>) => {
    if (command !== "chat_start") return [];
    starts++;
    if (starts <= 2) throw new DOMException("Request aborted", "AbortError");
    if (starts === 3 && !resumed) throw new Error("unexpected automatic retry");
    return { ...authority, subscription_id: args.subscriptionId, messages: [], text: "", start_offset: 0, offset: 0, pending: false };
  }) as never);
  function Probe({ active = true }: { active?: boolean }) {
    latest = useChatSession({ tabId: "aborted", agentSessionId: "own-session", sessionBindingNonce: "own-nonce", active, retainAttachment: true });
    return null;
  }
  await act(async () => root.render(<Probe />));
  expect(starts).toBe(2);
  expect(latest?.error).toBeNull();
  expect(latest?.attachmentState).not.toBe("failed");
  resumed = true;
  await act(async () => root.render(<Probe active={false} />));
  await act(async () => root.render(<Probe />));
  expect(starts).toBe(3);
  expect(latest?.attachmentState).toBe("attached");
});
it("reattaches after an aborted listener setup without reporting a terminal failure", async () => {
  failSnapshot = false;
  let listenerCalls = 0;
  __setListenForTest(async event => {
    if (event === "tab-chat-msg-failed-attach" && ++listenerCalls === 1) throw new DOMException("Listener aborted", "AbortError");
    return () => {};
  });
  await act(async () => root.render(<Harness />));
  expect(host.querySelector('[aria-label="Chat connection failed"]')).toBeNull();
  expect(host.querySelector('[role="log"]')?.textContent).toBe("Restored transcript");
  expect(listenerCalls).toBe(2);
});

it.each([new RetryableMuxError("/api/tabs/t/chat-raw/start", "Mux per-request buffer limit exceeded (600000 bytes; limit 524288 bytes)"),
  new OutcomeUnknown("POST", "/api/tabs/t/chat-raw/start", "SSE capacity exceeded")])("recovers an interrupted read snapshot once without a write-uncertainty banner: %s", async failure => {
  failSnapshot = false;
  __setInvokeForTest((async (command: string, args: Record<string, unknown>) => {
    if (command !== "chat_start") return [];
    starts++;
    if (starts === 1) throw failure;
    return { ...authority, subscription_id: args.subscriptionId, messages: [{ id: "row", type: "assistant", content: "Restored transcript" }], text: "", start_offset: 0, offset: 40, pending: false };
  }) as never);
  await act(async () => root.render(<Harness />));
  expect(host.querySelector('[aria-label="Chat connection failed"]')).toBeNull();
  expect(host.querySelector('[role="log"]')?.textContent).toBe("Restored transcript");
  expect(starts).toBe(2);
});
