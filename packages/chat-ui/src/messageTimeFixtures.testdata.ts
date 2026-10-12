import { reduceToolEvents, type ChatOrigin, type NormalizedChatMessage, type TimelineItem } from "@codeterm/chat-core";

export const FIRST_TIME = "2026-10-11T10:05:00.000Z";
export const LAST_TIME = "2026-10-11T10:08:00.000Z";
export interface TimeFixture {
  name: string;
  message: NormalizedChatMessage;
  origin?: ChatOrigin;
  item?: TimelineItem;
  streaming?: boolean;
  side: "left" | "right";
}
const message = (type: NormalizedChatMessage["type"], extra: Partial<NormalizedChatMessage> = {}): NormalizedChatMessage => ({ id: type, type, content: "fixture event", timestamp: FIRST_TIME, ...extra });
const user = (name: string, origin?: ChatOrigin, extra?: Partial<NormalizedChatMessage>): TimeFixture => ({ name, message: message("user", extra), origin, side: origin || extra?.inboundKind ? "left" : "right" });
const tool = (id: string, timestamp: string, toolInput = {}): NormalizedChatMessage => message("tool_call", {
  id, timestamp, toolId: id, toolKind: "command", toolName: "command", toolSummary: "echo fixture", toolInput: { command: "echo fixture", ...toolInput },
});
const batch = reduceToolEvents([tool("batch-first", FIRST_TIME, { commandBatchId: "batch" }), tool("batch-last", LAST_TIME, { commandBatchId: "batch" })])[0];
const stream = reduceToolEvents([
  { ...tool("stream-first", FIRST_TIME), toolFlowId: "flow", toolDescription: "fixture work" },
  { ...tool("stream-last", LAST_TIME), toolFlowId: "flow", toolDescription: "fixture work" },
])[0];
function grouped(name: string, item: TimelineItem): TimeFixture {
  if (item.kind !== "tool") throw new Error("fixture must aggregate tools");
  return { name, message: item.event.message, item, side: "left" };
}
export const timeFixtures: TimeFixture[] = [
  user("user"),
  user("attachment only", undefined, { content: "", attachments: [{ id: "file", mimeType: "text/plain", path: "/tmp/fixture.txt" }] }),
  user("received", { kind: "tab", title: "Sender", tabIdShort: "abcd1234" }),
  user("structured received", undefined, { inboundKind: "tab", fromTab: "abcd1234", fromLabel: "Sender" }),
  user("mesh", { kind: "tab", title: "Sender", tabIdShort: "abcd1234", mesh: "peer" }),
  user("report", { kind: "tab", title: "Sender", tabIdShort: "abcd1234", report: true }),
  user("reminder", { kind: "reminder", setAt: LAST_TIME, fireAt: LAST_TIME }),
  user("delayed send", undefined, { inboundKind: "reminder" }),
  user("automation", { kind: "automation", source: "cron", name: "Job" }),
  user("system notice", { kind: "system" }),
  user("system prompt", { kind: "system_prompt" }),
  user("plugin", { kind: "plugin", pluginId: "fixture", name: "Fixture" }),
  user("plugin settings", { kind: "plugin_config", pluginId: "fixture", name: "Fixture" }),
  ...(["assistant", "thinking", "tool_call", "tool_result", "status", "system", "local_command", "image", "shell_command", "context_request", "watcher_verdict"] as const).map((type): TimeFixture => ({ name: type, message: message(type), side: "left" })),
  { name: "rich card", message: message("assistant", { content: "```ds-plugin\n{\"id\":\"fixture\"}\n```" }), side: "left" },
  { name: "streaming", message: message("assistant"), streaming: true, side: "left" },
  { name: "compaction", message: message("custom", { timelineMarker: { kind: "context_compaction" } }), side: "left" },
  { name: "external edit", message: message("system", { timelineMarker: { kind: "external_edit", path: "/tmp/fixture.txt" } }), side: "left" },
  grouped("command batch", batch),
  grouped("tool stream", stream),
];
