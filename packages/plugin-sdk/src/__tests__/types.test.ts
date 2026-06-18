// Type-only conformance test. The oracle is `tsc --noEmit`: this file must
// compile when the contracts exist, and every `@ts-expect-error` line asserts a
// genuine type error (tsc fails if the error is absent — i.e. if a type is too
// loose). No runtime; nothing here is executed.

import type {
  ChatBackend,
  ChatPreprocessor,
  AgentProvider,
  AgentCommandHandler,
  AgentCommandCtx,
  Model,
  ModelInfo,
  NormalizedChatMessage,
} from "../capabilities";
import type { Host, Backend, PaneRef, UsageSnapshot } from "../host";

// ── ChatBackend: the four required methods + optional listModels ──
const backend: ChatBackend = {
  openSession: (ctx) => ({ sessionId: ctx.paneId }),
  sendMessage: (_sessionId, _text) => {},
  poll: (_sessionId, _cursor) => ({ messages: [], cursor: null }),
  closeSession: (_sessionId) => {},
  listModels: () => [{ id: "m", displayName: "M" }],
};
void backend;

// listModels is optional — omitting it still satisfies ChatBackend.
const minimalBackend: ChatBackend = {
  openSession: () => ({ sessionId: "s" }),
  sendMessage: () => {},
  poll: () => ({ messages: [], cursor: "c", done: true }),
  closeSession: () => {},
};
void minimalBackend;

// A missing required method is a type error.
// @ts-expect-error closeSession is required
const brokenBackend: ChatBackend = {
  openSession: () => ({ sessionId: "s" }),
  sendMessage: () => {},
  poll: () => ({ messages: [], cursor: null }),
};
void brokenBackend;

// ── NormalizedChatMessage: minimal { id, type, content } ──
const msg: NormalizedChatMessage = { id: "1", type: "assistant", content: "hi" };
void msg;
const polled: NormalizedChatMessage[] = backend.poll("s").messages;
void polled;

// ── ChatPreprocessor: match slot + passthrough (null) ──
const pre: ChatPreprocessor = {
  match: "text",
  chatPreprocess: (ctx) =>
    ctx.text.length > 0 ? { text: ctx.text.toUpperCase() } : null,
};
void pre;

// match is the literal "text" — anything else is a type error.
const wrongSlot: ChatPreprocessor = {
  // @ts-expect-error match must be the literal "text"
  match: "voice",
  chatPreprocess: () => null,
};
void wrongSlot;

// ── AgentCommandHandler / AgentCommandCtx: the inverse seam ──
const cmd: AgentCommandHandler = {
  onAgentCommand: (ctx: AgentCommandCtx) =>
    ctx.verb === "ping"
      ? { result: ctx.args.join(",") }
      : { error: "unknown verb" },
};
void cmd;

// ── Model: id + camelCase displayName ──
const model: Model = { id: "gpt", displayName: "GPT" };
void model;
// @ts-expect-error SDK uses camelCase displayName, not snake_case display_name
const snakeModel: Model = { id: "gpt", display_name: "GPT" };
void snakeModel;

// ── AgentProvider: optional live model discovery ──
const provider: AgentProvider = {
  discoverModels: () => [{ id: "gpt-5.5", displayName: "GPT-5.5", description: "live" }],
};
void provider;
const providerModel: ModelInfo = provider.discoverModels!()[0];
void providerModel;
// @ts-expect-error discoverModels returns the shared camelCase ModelInfo shape
const brokenProvider: AgentProvider = { discoverModels: () => [{ id: "x", display_name: "X" }] };
void brokenProvider;

// ── host compute base: worker/workspace/agent/mem (job-id + poll) ──
declare const host: Host;

async function exercise() {
  // worker: non-blocking start → poll (F4), not blocking run.
  const { jobId } = await host.worker.start({ task: "do it" });
  const w = await host.worker.poll(jobId);
  if (w.done) void w.report;
  void w.error;

  // workspace: ensure + panes — SYNCHRONOUS (cheap mux ops, no agent turn).
  const { workspaceId } = host.workspace.ensure({ name: "ws" });
  const panes: PaneRef[] = host.workspace.panes(workspaceId);
  void panes;

  // agent: spawn (fresh) / resume (specific) / get (key→session) / send (ticket) / poll / reap / usage.
  const backendSpec: Backend = { provider: "claude", model: "opus" };
  const { sessionId } = await host.agent.spawn(workspaceId, {
    backend: backendSpec,
    task: "play npc",
    key: "npc_1",
    commands: ["say", "move"],
  });
  const resumed = await host.agent.resume(workspaceId, sessionId);
  void resumed;
  const found = await host.agent.get(workspaceId, "npc_1");
  void found;
  const { ticket } = await host.agent.send(sessionId, "hello");
  const reply = await host.agent.poll(ticket);
  if (reply.done) void reply.reply;
  void reply.error;
  const usage: UsageSnapshot | null = host.agent.usage
    ? await host.agent.usage(sessionId)
    : null;
  void usage;
  await host.agent.reap(sessionId);

  // mem: SYNCHRONOUS search (callable mid-turn by a sync stage), scored hits.
  const { hits } = host.mem.search({ query: "q", k: 5 });
  void hits.map((h) => `${h.id}:${h.text}:${h.score}`);

  // fetch streaming: same sync job-id + poll shape as worker/agent.
  const rawFetchResult = JSON.parse(host.fetch(JSON.stringify({ url: "http://127.0.0.1" })));
  void rawFetchResult;
  const stream = host.fetch.stream({ url: "http://127.0.0.1/stream" });
  if (stream.jobId) {
    const streamPoll = host.fetch.streamPoll(stream.jobId);
    void streamPoll.chunks.join("");
    void streamPoll.status;
    void streamPoll.error;
    host.fetch.streamClose(stream.jobId);
  }
}
void exercise;
