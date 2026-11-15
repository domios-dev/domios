// Type-only conformance test. The oracle is `tsc --noEmit`: this file must
// compile when the contracts exist, and every `@ts-expect-error` line asserts a

import type {
  ChatBackend,
  ChatPreprocessor,
  DecisionAnswer,
  DecisionQuestionsRequest,
  DecisionResponse,
  DecisionModelCapability,
  DecisionModelInfo,
  AgentProvider,
  AgentCommandHandler,
  AgentCommandCtx,
  Model,
  ModelInfo,
  NormalizedChatMessage,
  PluginModule,
  SurfaceContribution,
  TabMenuContribution,
  ViewHostAction,
  ViewHostBridge,
  ViewHostResult,
  ViewInputsContribution,
  WebhookDelivery,
  WebhookReceiveContext,
  WebhookReceiverCapability,
  WebhookReceiver,
  LocalRecordingTriggerCapability,
} from "./capabilities";
import type { Host, Backend } from "./host";

const localTrigger: LocalRecordingTriggerCapability = {
  vendorId: 1, productId: 2, usagePage: 3, usage: 4,
};
void localTrigger;

function* localRecordingConformance(host: Host): import("./io").HostIoSteps<void> {
  yield {op:"localRecordingStatus"};
  yield {op:"localRecordingSetEnabled",enabled:true};
  yield {op:"receiverStatusReport"};
  // @ts-expect-error status request bytes belong to the manifest
  yield {op:"receiverStatusReport",request:[1],path:"foreign"};
  // @ts-expect-error device selection remains host-owned
  yield {op:"localRecordingSelect",devices:{inputId:"input"}};
  // @ts-expect-error synchronous recording APIs have been removed
  host.localRecording.start();
}
void localRecordingConformance;

const backend: ChatBackend = {
  openSession: (ctx) => ({ sessionId: ctx.tabId }),
  sendMessage: (_sessionId, _text) => {},
  poll: (_sessionId, _cursor) => ({ messages: [], cursor: null }),
  closeSession: (_sessionId) => {},
  listModels: () => [{ id: "m", displayName: "M" }],
  listPresets: () => [{ id: "codeterm", name: "CodeTerm", description: "default shell" }],
};
void backend;

const openShellSession = backend.openSession({
  tabId: "tab-1",
  config: {},
  systemPrompt: "You are CodeTerm.",
  preset: "codeterm",
  model: "qwen",
});
void openShellSession;

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

const msg: NormalizedChatMessage = {
  id: "1",
  type: "assistant",
  content: "hi",
};
void msg;
const pluginInbound: NormalizedChatMessage = {
  id: "plugin-1",
  type: "user",
  content: "hello",
  inboundKind: "plugin",
  fromPlugin: "pebble",
  pluginIconSvg: "<svg />",
};
void pluginInbound;
const polled: NormalizedChatMessage[] = backend.poll("s").messages;
void polled;

const pre: ChatPreprocessor = {
  match: "text",
  chatPreprocess: (ctx) => (ctx.text.length > 0 ? { text: ctx.text.toUpperCase() } : null),
};
void pre;

const webhookContext: WebhookReceiveContext = {
  payload: { event_id: "evt-1" },
  receivedAt: "2026-09-11T07:00:00Z",
};
const delivery: WebhookDelivery = { text: "Pebble event", eventId: "evt-1" };
const receiver: WebhookReceiver = {
  webhookReceive: (ctx) => (ctx.payload ? delivery : null),
};
void webhookContext;
void receiver;

const audioContext: WebhookReceiveContext = {
  payload: {},
  receivedAt: "2026-09-11T07:00:00Z",
  audio: { mediaType: "audio/mp4", size: 3 },
};
const audioDelivery: WebhookDelivery = {
  text: "client: ring",
  transcript: "hello",
};
const audioCapability: WebhookReceiverCapability = {
  kind: "pebble",
  audio: true,
};
void audioContext;
void audioDelivery;
void audioCapability;

const badTranscript: WebhookDelivery = {
  text: "ok",
  // @ts-expect-error transcript must be a string
  transcript: 7,
};
void badTranscript;

const plugin: PluginModule = {
  webhookReceive: () => ({ text: "Pebble event" }),
};
void plugin;

// The receiver is synchronous and may intentionally suppress delivery.
const synchronousNoOp: WebhookReceiver = {
  webhookReceive: () => null,
};
void synchronousNoOp;

const malformedReceiver: WebhookReceiver = {
  // @ts-expect-error webhookReceive must return a normalized delivery or null
  webhookReceive: () => ({ message: "not a delivery" }),
};
void malformedReceiver;

// match is the literal "text" — anything else is a type error.
const wrongSlot: ChatPreprocessor = {
  // @ts-expect-error match must be the literal "text"
  match: "voice",
  chatPreprocess: () => null,
};
void wrongSlot;

const cmd: AgentCommandHandler = {
  onAgentCommand: (ctx: AgentCommandCtx) =>
    ctx.verb === "ping"
      ? { result: ctx.args.join(","), catalogueChanged: true }
      : { error: "unknown verb" },
};
void cmd;

const model: Model = { id: "gpt", displayName: "GPT" };
void model;
// @ts-expect-error SDK uses camelCase displayName, not snake_case display_name
const snakeModel: Model = { id: "gpt", display_name: "GPT" };
void snakeModel;

const provider: AgentProvider = {
  discoverModels: () => [{ id: "gpt-5.5", displayName: "GPT-5.5", description: "live" }],
};
void provider;
const providerModel: ModelInfo = provider.discoverModels!()![0];
void providerModel;
const brokenProvider: AgentProvider = {
  // @ts-expect-error discoverModels returns the shared camelCase ModelInfo shape
  discoverModels: () => [{ id: "x", display_name: "X" }],
};
void brokenProvider;

declare const host: Host;

const decisionModel: DecisionModelCapability = {
  primitives: () => ({ choice: true, noul: true, score: true }),
  models: () => [{ id: "loaded", display_name: "Loaded model" }],
  decide(request): DecisionAnswer {
    if (request.question.type === "noul") return { type: "noul", p: 0.8 };
    if (request.question.type === "choice") {
      return {
        type: "choice",
        choice: "yes",
        probabilities: { yes: 0.8, no: 0.2 },
        confidence: 0.8,
      };
    }
    return {
      type: "score",
      score: 2,
      probabilities: { "0": 0.1, "1": 0.2, "2": 0.7 },
      confidence: 0.7,
    };
  },
};
const sharedDecisionModel: DecisionModelCapability = {
  ...decisionModel,
  pricing: () => ({
    display_name: "local fixture",
    input_usd_per_million: 0,
    output_usd_per_million: 0,
  }),
  decideQuestions(request: DecisionQuestionsRequest): DecisionResponse {
    return {
      answers: Object.fromEntries(
        Object.entries(request.questions).map(([name, spec]) => {
          const answer = decisionModel.decide({
            state: request.state,
            instructions: spec.instructions,
            question: spec.question,
          });
          if ("error" in answer) throw new Error(answer.error);
          return [name, answer];
        }),
      ),
      usage: { input_tokens: 12, output_tokens: 4 },
    };
  },
};
void sharedDecisionModel;

const decisionModelInfo: DecisionModelInfo = {
  id: "loaded",
  display_name: "Loaded model",
};
const decisionPlugin: PluginModule = { ...decisionModel };
const decisionAnswer: DecisionAnswer = {
  type: "noul",
  p: 0.6,
};
void decisionPlugin;
void decisionModelInfo;
void decisionAnswer;

// Type-only check: `host` exists only inside the plugin VM, so this is never called here.
function typeCheckAsyncFetch(): string {
  return host.fetch.async(
    { url: "http://127.0.0.1/decision" },
    (result) => result.body ?? result.error ?? String(result.status),
  );
}
void typeCheckAsyncFetch;

function pureFsConformance(host: Host) {
  const path: string | null = host.fs.expandHome("~/.domios");
  void path;
  // @ts-expect-error synchronous filesystem I/O remains removed
  host.fs.readFile("foreign");
}
void pureFsConformance;

function* exercise(): import("./io").HostIoSteps<void> {
  yield {op:"workerStart",options:{task:"do it"}};
  yield {op:"workerPoll",job:"owned-job"};
  yield {op:"workspaceEnsure",options:{name:"ws"}};
  yield {op:"workspaceTabs",workspace:"ws"};
  const backendSpec: Backend = {provider:"claude",model:"opus"};
  yield {op:"agentSpawn",workspace:"ws",options:{...backendSpec,task:"play npc"}};
  yield {op:"agentSend",session:"owned-session",text:"hello"};
  yield {op:"agentPoll",session:"owned-session"};
  yield {op:"agentReap",session:"owned-session"};
  yield {op:"memSearch",options:{query:"q",k:5}};
  yield* host.io.fetch({url:"http://127.0.0.1"});
  yield {op:"fetchStreamStart",options:{url:"http://127.0.0.1/stream"}};
  yield {op:"fetchStreamPoll",job:"owned-stream"};
  yield {op:"fetchStreamClose",job:"owned-stream"};
  yield* host.io.exec({bin:"echo",args:["hi"]});
  yield {op:"execPoll",job:"owned-exec"};
  yield {op:"execClose",job:"owned-exec"};
  yield {op:"receiverTarget"};
  yield {op:"receiverTargetClear"};
  // @ts-expect-error receiver assignment remains host-owned
  yield {op:"receiverTargetSet",tab:"tab-1"};
}
void exercise;

const receiverMenu: TabMenuContribution = {
  id: "receiver-target",
  label: "Receive events here",
  action: "receiverTarget",
  icon: "icon.svg",
};
void receiverMenu;
const unknownAction: TabMenuContribution = {
  id: "close",
  label: "Close",
  // @ts-expect-error tab menu actions are a closed host-owned set
  action: "closeTab",
};
void unknownAction;

const boardSurface: SurfaceContribution = {
  id: "board",
  title: "Board",
  renderer: "view",
  agentBound: true,
  restore: "none",
};
void boardSurface;
const streamSurface: SurfaceContribution = {
  id: "browser",
  title: "Browser",
  renderer: "stream",
  runtime: "browser",
  chrome: { items: [] },
};
void streamSurface;
const unknownRenderer: SurfaceContribution = {
  id: "screen",
  title: "Screen",
  // @ts-expect-error renderers are a closed set
  renderer: "canvas",
};
void unknownRenderer;
const declaredHostActions: ViewHostAction[] = ["copyText", "saveAs", "reveal", "pickFile"];
void declaredHostActions;
const referenceInputs: ViewInputsContribution = { accept: ["image/png", "image/jpeg"], maxBytes: 20_971_520, maxFiles: 4 };
void referenceInputs;
// @ts-expect-error only passive media may be picked
const svgInputs: ViewInputsContribution = { accept: ["image/svg+xml"], maxBytes: 1, maxFiles: 1 };
void svgInputs;
async function pickReference(bridge: ViewHostBridge): Promise<string[]> {
  const result = await bridge.pickFile({ accept: ["image/png"], multiple: true });
  return result.ok ? result.inputs.map((input) => input.handle) : [];
}
void pickReference;
// @ts-expect-error host actions are a closed set
const openUrlAction: ViewHostAction = "openUrl";
void openUrlAction;
async function saveFromView(bridge: ViewHostBridge): Promise<string> {
  if (!(await bridge.availability()).saveAs) return "hidden";
  const result: ViewHostResult = await bridge.saveAs("0".repeat(64), "frame.png");
  return result.ok ? "saved" : `${result.code}:${result.reason}`;
}
void saveFromView;
function callSurface(): string {
  return host.io.run(host.io.surface.call("status", "[]"));
}
void callSurface;

const lifecycleModule: PluginModule = {
  onPermissionChanged(event) {
    const permission: string = event.permission;
    const allowed: true = event.allowed;
    const source: "install" | "action" | "settings" = event.source;
    void [permission, allowed, source];
  },
};
void lifecycleModule;

import type {AgentRuntime, RuntimeContext, RuntimeAction, AgentEvent, HostAgentEvent, StdioAgentRuntimeCapability, RuntimeRestoreContext, Operation} from "./index";
function runtimeConformance(runtime:AgentRuntime, context:RuntimeContext) {
  const restored:RuntimeRestoreContext={instructions:"opaque",additionalContext:[{key:"opaque",kind:"application",value:"opaque"}]};
  const restore:Operation={kind:"control",action:"restoreContext",value:restored};
  void restore;
  const plan=runtime.runtimePlan(context,{kind:"initialize"});
  const action:RuntimeAction=plan.action;
  const event:AgentEvent={kind:"turnState",threadId:"own",state:"working",startedAt:1000};
  const hostEvent:HostAgentEvent={...event,at:2000};
  const descriptor:StdioAgentRuntimeCapability={runtime:"stdio-jsonrpc",command:{binary:"agent",args:[]},requestMethods:{initialize:["hello"]},notificationMethods:[]};
  // @ts-expect-error Process handles are not in runtime context.
  context.process.kill();
  // @ts-expect-error The host receipt stamp is not produced by pure hooks.
  const invalid:AgentEvent={kind:"turnState",threadId:"own",state:"idle",at:1};
  void [action,event,hostEvent,descriptor,invalid];
}
void runtimeConformance;

function commandBodyConformance(ctx: AgentCommandCtx) {
  const body: string | undefined = ctx.body;
  const omitted: AgentCommandCtx = { sessionId: "s", verb: "send", args: [] };
  const supplied: AgentCommandCtx = { ...omitted, body: "--message\r\nλ😀\n" };
  // @ts-expect-error Command bodies are text.
  const invalid: AgentCommandCtx = { ...omitted, body: 1 };
  void [body, supplied, invalid];
}
void commandBodyConformance;
