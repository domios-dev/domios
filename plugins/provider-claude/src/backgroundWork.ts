import type { AgentProvider } from "@codeterm/plugin-sdk";

type BackgroundWorkObservation = ReturnType<NonNullable<AgentProvider["parseBackgroundWork"]>>[number];

const TASK_ID = /^[A-Za-z0-9_-]{1,256}$/;
const END_STATES = ["completed", "killed", "failed"] as const;

function object(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function shellOutputPath(content: unknown, session: unknown, id: string): string | undefined {
  if (!Array.isArray(content) || typeof session !== "string") return;
  const prefix = `Command running in background with ID: ${id}. Output is being written to: `;
  for (const value of content) {
    const block = object(value);
    if (block?.type !== "tool_result" || block.is_error === true || typeof block.content !== "string") continue;
    if (!block.content.startsWith(prefix)) continue;
    const end = block.content.indexOf(". You will be notified", prefix.length);
    if (end < 0) continue;
    const path = block.content.slice(prefix.length, end);
    const [root, key, owner, directory, filename] = path.split(/[\\/]/).slice(-5);
    return /^claude(?:-[0-9]+)?$/.test(root ?? "") && key && owner === session
      && directory === "tasks" && filename === `${id}.output` ? path : undefined;
  }
}

export function parseBackgroundWork(value: unknown): BackgroundWorkObservation[] {
  const row = object(value);
  if (!row) return [];
  if (row.type === "summary" || row.isCompactSummary === true
    || (row.type === "system" && ["compact_boundary", "init", "resume"].includes(String(row.subtype)))) {
    return [{ id: "", state: "session_boundary" }];
  }
  const result = object(row.toolUseResult);
  const content = object(row.message)?.content;
  const events: BackgroundWorkObservation[] = [];
  const shellId = result?.backgroundTaskId;
  if (typeof shellId === "string" && TASK_ID.test(shellId)) {
    const outputPath = shellOutputPath(content, row.sessionId, shellId);
    events.push({ id: shellId, state: "running", kind: "shell", ...(outputPath && { output_path: outputPath }) });
  }
  const agentId = result?.agentId;
  const agentStatus = result?.status === "async_launched" ? "running" : result?.status;
  if (typeof agentId === "string" && TASK_ID.test(agentId)
    && (agentStatus === "running" || agentStatus === "completed" || agentStatus === "killed" || agentStatus === "failed")) {
    events.push({ id: agentId, state: agentStatus, kind: "subagent" });
  }
  const stoppedId = result?.task_id;
  if (row.type === "user" && result?.task_type === "local_bash" && typeof result.command === "string"
    && typeof stoppedId === "string" && TASK_ID.test(stoppedId)
    && !(Array.isArray(content) && content.some(block => object(block)?.is_error === true))) {
    events.push({ id: stoppedId, state: "killed", kind: "shell" });
  }
  const attachment = object(row.attachment);
  const notification = row.type === "attachment" && attachment?.type === "queued_command"
    && attachment.commandMode === "task-notification"
    ? attachment.prompt
    : row.type === "user" && row.isMeta === true ? content : undefined;
  if (typeof notification === "string") {
    for (const block of notification.match(/<task-notification>[\s\S]*?<\/task-notification>/g) ?? []) {
      const id = /<task-id>([^<]+)<\/task-id>/.exec(block)?.[1].trim();
      const status = /<status>([^<]+)<\/status>/.exec(block)?.[1].trim();
      const state = END_STATES.find(state => state === status);
      if (id && TASK_ID.test(id) && state) events.push({ id, state });
    }
  }
  return events;
}
