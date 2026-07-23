// Run: CODETERM_INSTANCE=chat-stale-h123 npx tsx src/components/Terminal/useChatSession.causal.test.tsx
// Failing freshness contract: set CHAT_STALE_EXPECT_FRESH=1 on the same command.
import type { NormalizedChatMessage } from "@codeterm/chat-core";
import {
  act,
  createElement,
  useDeferredValue,
  useLayoutEffect,
  useState,
} from "react";
import { flushSync } from "react-dom";
import { createRoot, type Root } from "react-dom/client";
import { Window } from "happy-dom";
import type {
  ChatSessionDiagnosticEvent,
  ChatSessionDiagnosticSink,
} from "./useChatSession";

const INSTANCE = process.env.CODETERM_INSTANCE ?? "chat-stale-h123";
const EXPECT_FRESH = process.env.CHAT_STALE_EXPECT_FRESH === "1";
const MAX_TIMELINE = 256;

function assert(condition: unknown, label: string): asserts condition {
  if (!condition) throw new Error(label);
}

function setupHookDom(): void {
  const hookWindow = new Window({ url: "http://localhost/" });
  Object.defineProperty(globalThis, "IS_REACT_ACT_ENVIRONMENT", {
    configurable: true,
    value: true,
    writable: true,
  });
  Object.defineProperty(globalThis, "window", { configurable: true, value: hookWindow });
  Object.defineProperty(globalThis, "document", { configurable: true, value: hookWindow.document });
  Object.defineProperty(globalThis, "navigator", { configurable: true, value: hookWindow.navigator });
  Object.defineProperty(globalThis, "HTMLElement", {
    configurable: true,
    value: hookWindow.HTMLElement,
  });
  Object.defineProperty(globalThis, "Event", { configurable: true, value: hookWindow.Event });
  Object.defineProperty(globalThis, "CustomEvent", {
    configurable: true,
    value: hookWindow.CustomEvent,
  });
}

type Edge = ChatSessionDiagnosticEvent["edge"] | "server_delta" | "server_eof" | "visible_commit";

interface TimelineEntry {
  seq: number;
  edge: Edge;
  sourceSessionId?: string | null;
  sessionBindingNonce?: string | null;
  messageCount?: number;
  firstMessageId?: string;
  lastMessageId?: string;
  subscriptionCount?: number;
}

class BoundedTimeline {
  readonly entries: TimelineEntry[] = [];
  private sequence = 0;

  push(entry: Omit<TimelineEntry, "seq">): void {
    this.entries.push({ seq: this.sequence, ...entry });
    this.sequence += 1;
    if (this.entries.length > MAX_TIMELINE) this.entries.shift();
  }

  diagnostic: ChatSessionDiagnosticSink = (event) => {
    this.push({
      edge: event.edge,
      sourceSessionId: "sourceSessionId" in event ? event.sourceSessionId : undefined,
      sessionBindingNonce:
        "sessionBindingNonce" in event ? event.sessionBindingNonce : undefined,
      messageCount: "messageIds" in event ? event.messageIds.length : undefined,
      firstMessageId: "messageIds" in event ? event.messageIds[0] : undefined,
      lastMessageId:
        "messageIds" in event ? event.messageIds[event.messageIds.length - 1] : undefined,
    });
  };

  count(edge: Edge): number {
    return this.entries.filter((entry) => entry.edge === edge).length;
  }
}

type ChatHandler = (event: { payload: unknown }) => void;

class FakeChatServer {
  readonly subscribers = new Set<string>();
  readonly timeline: BoundedTimeline;
  handler: ChatHandler | null = null;
  sourceSessionId = "session-own";
  sessionBindingNonce = "nonce-a";
  snapshot: NormalizedChatMessage[] = [];
  snapshotReads = 0;
  deltaReads = 0;
  eofSignals = 0;
  listens = 0;
  unlistens = 0;
  peakSubscriptions = 0;

  constructor(timeline: BoundedTimeline) {
    this.timeline = timeline;
  }

  install(bridge: {
    __setInvokeForTest: (fn: unknown) => void;
    __setListenForTest: (fn: unknown) => void;
  }): void {
    bridge.__setListenForTest(async (_event: string, nextHandler: ChatHandler) => {
      this.listens += 1;
      assert(this.handler == null, "H3 falsifier: a second live listener was acquired");
      this.handler = nextHandler;
      return () => {
        this.unlistens += 1;
        if (this.handler === nextHandler) this.handler = null;
      };
    });
    bridge.__setInvokeForTest(async (command: string, args: Record<string, unknown>) => {
      if (command === "chat_start") {
        this.snapshotReads += 1;
        const subscriptionId = String(args.subscriptionId ?? "");
        this.subscribers.add(subscriptionId);
        this.peakSubscriptions = Math.max(this.peakSubscriptions, this.subscribers.size);
        return {
          text: "",
          start_offset: 0,
          offset: this.snapshot.length,
          pending: false,
          messages: [...this.snapshot],
          source_session_id: this.sourceSessionId,
          session_binding_nonce: this.sessionBindingNonce,
          subscription_id: subscriptionId,
        };
      }
      if (command === "chat_stop") {
        this.subscribers.delete(String(args.subscriptionId ?? ""));
        return {};
      }
      return {};
    });
  }

  emit(
    messages: NormalizedChatMessage[],
    sourceSessionId = this.sourceSessionId,
    sessionBindingNonce = this.sessionBindingNonce,
  ): void {
    this.deltaReads += 1;
    this.timeline.push({
      edge: "server_delta",
      sourceSessionId,
      sessionBindingNonce,
      messageCount: messages.length,
      firstMessageId: messages[0]?.id,
      lastMessageId: messages[messages.length - 1]?.id,
      subscriptionCount: this.subscribers.size,
    });
    this.handler?.({
      payload: {
        pane_id: "pane",
        messages,
        source_session_id: sourceSessionId,
        session_binding_nonce: sessionBindingNonce,
      },
    });
  }

  eof(): void {
    this.eofSignals += 1;
    this.timeline.push({
      edge: "server_eof",
      sourceSessionId: this.sourceSessionId,
      sessionBindingNonce: this.sessionBindingNonce,
      subscriptionCount: this.subscribers.size,
    });
  }
}

function msg(id: string): NormalizedChatMessage {
  return { id, type: "assistant", content: id, timestamp: id.slice(1) || "0" };
}

async function flush(): Promise<void> {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
}

interface Mounted {
  root: Root;
  render: (sessionId: string, nonce: string) => Promise<void>;
  bumpUrgent: () => void;
  committedStateCount: () => number;
  visibleCount: () => number;
}

async function mount(
  server: FakeChatServer,
  timeline: BoundedTimeline,
): Promise<Mounted> {
  const bridge = await import("../../lib/bridge");
  const { useChatSession } = await import("./useChatSession");
  server.install(bridge as never);

  let committedStateCount = 0;
  let visibleCount = 0;
  let urgentUpdate: (() => void) | null = null;

  function Harness({ sessionId, nonce }: { sessionId: string; nonce: string }) {
    const [urgentEpoch, setUrgentEpoch] = useState(0);
    urgentUpdate = () => setUrgentEpoch((value) => value + 1);
    const session = useChatSession({
      paneId: "pane",
      agentSessionId: sessionId,
      sessionBindingNonce: nonce,
      active: true,
      diagnosticSink: timeline.diagnostic,
    });
    const deferred = useDeferredValue(session.enrichedMessages);
    useLayoutEffect(() => {
      committedStateCount = session.allMessages.length;
    }, [session.allMessages]);
    useLayoutEffect(() => {
      visibleCount = deferred.length;
      timeline.push({
        edge: "visible_commit",
        messageCount: deferred.length,
        firstMessageId: deferred[0]?.msg.id,
        lastMessageId: deferred[deferred.length - 1]?.msg.id,
      });
    }, [deferred]);
    return createElement("output", { "data-urgent": urgentEpoch }, String(deferred.length));
  }

  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  const render = async (sessionId: string, nonce: string) => {
    await act(async () => {
      root.render(createElement(Harness, { sessionId, nonce }));
    });
    await flush();
  };
  await render(server.sourceSessionId, server.sessionBindingNonce);
  return {
    root,
    render,
    bumpUrgent: () => urgentUpdate?.(),
    committedStateCount: () => committedStateCount,
    visibleCount: () => visibleCount,
  };
}

async function main(): Promise<void> {
  setupHookDom();
  assert(
    INSTANCE === "chat-stale-h123",
    `isolated capture requires CODETERM_INSTANCE=chat-stale-h123 (got ${INSTANCE})`,
  );

  const timeline = new BoundedTimeline();
  const server = new FakeChatServer(timeline);
  server.snapshot = [msg("m0")];
  const app = await mount(server, timeline);
  assert(server.snapshotReads === 1, "attach performs exactly one snapshot read");
  assert(server.subscribers.size === 1, "H3 falsifier: attach owns one subscription");

  // T-H2: the source session remains authoritative across a nonce rotation.
  await act(async () => {
    server.emit([msg("m1")], server.sourceSessionId, "nonce-rotated");
  });
  assert(timeline.count("authority_drop") === 0, "H2 falsifier: own-session rotated nonce dropped");
  assert(timeline.count("authority_adopt") === 1, "H2 falsifier: own-session delta was not adopted");
  assert(app.visibleCount() === 2, "H2 falsifier: adopted own-session delta did not project");
  assert(server.snapshotReads === 1, "steady-state delta triggered a forbidden snapshot");

  server.sourceSessionId = "session-next";
  server.sessionBindingNonce = "nonce-b";
  server.snapshot = [msg("m0"), msg("m1"), msg("m2")];
  await app.render(server.sourceSessionId, server.sessionBindingNonce);
  assert(Number(server.snapshotReads) === 2, "an actual session roll did not perform one snapshot");
  assert(server.subscribers.size === 1, "session roll duplicated the live subscription");

  // T-H3: event-driven EOF is a no-read edge and ownership remains singular.
  const readsAtEof = server.deltaReads;
  server.eof();
  await flush();
  assert(server.deltaReads === readsAtEof, "EOF performed a provider read");
  assert(Number(server.snapshotReads) === 2, "EOF triggered a snapshot read");

  // T-H1: urgent commits keep winning while every transcript delta is queued
  // in the transition lane; release produces one state/projection batch.
  const baselineStateCommits = timeline.count("state_commit");
  const baselineVisibleCommits = timeline.count("visible_commit");
  const baselineVisibleCount = app.visibleCount();
  const streamCount = 48;
  let stateCommitsDuringLoad = -1;
  let visibleCommitsDuringLoad = -1;
  await act(async () => {
    for (let index = 0; index < streamCount; index += 1) {
      server.emit([msg(`m${index + 3}`)]);
      flushSync(app.bumpUrgent);
      if (EXPECT_FRESH) {
        assert(
          app.visibleCount() === baselineVisibleCount + index + 1,
          `T-H1 stale projection after delta ${index + 1}: visible=${app.visibleCount()} expected=${baselineVisibleCount + index + 1}`,
        );
      }
      assert(
        app.visibleCount() === baselineVisibleCount,
        "H1 falsifier: projection advanced while urgent work continuously preempted it",
      );
    }
    stateCommitsDuringLoad = timeline.count("state_commit") - baselineStateCommits;
    visibleCommitsDuringLoad = timeline.count("visible_commit") - baselineVisibleCommits;
  });

  assert(
    timeline.count("event_arrival") === streamCount + 1,
    "H3 falsifier: server delta did not reach the live listener",
  );
  assert(
    timeline.count("authority_adopt") === streamCount + 1,
    "H2 falsifier: an own-session stream delta lost authority",
  );
  assert(
    stateCommitsDuringLoad === 0,
    "H1 boundary moved past state commit during urgent load",
  );
  assert(
    visibleCommitsDuringLoad === 0,
    "H1 boundary moved past visible projection during urgent load",
  );

  assert(
    app.committedStateCount() === streamCount + 3,
    "deferred state did not commit the complete stream batch",
  );
  assert(
    app.visibleCount() === streamCount + 3,
    "visible projection did not catch up as one complete batch",
  );
  assert(
    timeline.count("state_commit") === baselineStateCommits + 1,
    "stream should collapse into one state commit after urgent load ends",
  );
  assert(
    timeline.count("visible_commit") === baselineVisibleCommits + 1,
    "stream should collapse into one visible commit after urgent load ends",
  );
  assert(server.peakSubscriptions === 1, "H3 falsifier: overlapping live subscriptions observed");
  assert(server.subscribers.size === 1, "H3 falsifier: live subscription was lost");
  assert(Number(server.snapshotReads) === 2, "streaming steady state fell back to a snapshot");

  await act(async () => app.root.unmount());
  await flush();
  assert(Number(server.subscribers.size) === 0, "unmount leaked the live subscription");
  assert(server.listens === 1 && server.unlistens === 1, "listener ownership was not one-in/one-out");

  const summary = {
    instance: INSTANCE,
    classification: "H1",
    counters: {
      serverDeltas: server.deltaReads,
      eventArrivals: timeline.count("event_arrival"),
      authorityAdopts: timeline.count("authority_adopt"),
      authorityDrops: timeline.count("authority_drop"),
      stateCommitsDuringLoad,
      stateCommitsAfterRelease: 1,
      visibleCommitsDuringLoad,
      visibleCommitsAfterRelease: 1,
      snapshots: server.snapshotReads,
      eofSignals: server.eofSignals,
      peakSubscriptions: server.peakSubscriptions,
    },
    falsifiers: {
      "T-H1": "visible projection advances during continuous urgent work",
      "T-H2": "an own-session delta is rejected after nonce rotation",
      "T-H3": "arrival count diverges or live subscription cardinality differs from one",
    },
    timelineTail: timeline.entries.slice(-24),
  };
  console.log(JSON.stringify(summary, null, 2));
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
