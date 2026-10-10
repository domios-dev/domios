import { existsSync, readFileSync, readdirSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";

const text = (value) => typeof value === "string" ? value : Array.isArray(value)
  ? value.map(text).join("\n") : text(value?.text ?? value?.content ?? "");
const parseLines = (value) => value.split("\n").filter(Boolean).flatMap((line) => {
  try { return [JSON.parse(line)]; } catch { return []; }
});
const open = (path) => new DatabaseSync(path, { readOnly: true });

export function recordBoundary(provider, cwd, session) {
  let path;
  if (provider === "claude") {
    path = join(process.env.CLAUDE_CONFIG_DIR || join(homedir(), ".claude"), "projects",
      cwd.replace(/[^a-zA-Z0-9]/g, "-"), `${session}.jsonl`);
  } else if (provider === "grok") {
    path = join(homedir(), ".grok/sessions", encodeURIComponent(cwd), session, "updates.jsonl");
  } else if (provider === "codex") {
    const root = process.env.CODEX_HOME || join(homedir(), ".codex");
    for (const name of readdirSync(root).filter((name) => /^state_\d+\.sqlite$/.test(name)).sort().reverse()) {
      const db = open(join(root, name));
      try { path = db.prepare("SELECT rollout_path FROM threads WHERE id = ?").get(session)?.rollout_path; }
      finally { db.close(); }
      if (path) break;
    }
  } else if (provider === "opencode") {
    path = process.env.OPENCODE_DB || join(process.env.XDG_DATA_HOME || join(homedir(), ".local/share"), "opencode/opencode.db");
    const db = open(path);
    try {
      const rows = db.prepare("SELECT id FROM message WHERE session_id = ? ORDER BY time_created, rowid").all(session);
      return { provider, session, path, message_ids: rows.map((row) => row.id) };
    } finally { db.close(); }
  }
  if (!path || !existsSync(path)) throw new Error(`Own ${provider} persisted record is unavailable for ${session}`);
  return { provider, session, path, bytes: readFileSync(path).length };
}

export function nextTurnModel(boundary, marker) {
  const { provider, session, path } = boundary;
  if (provider === "opencode") {
    const db = open(path);
    try {
      const rows = db.prepare("SELECT id, data FROM message WHERE session_id = ? ORDER BY time_created, rowid").all(session);
      const newRows = rows.filter((row) => !boundary.message_ids.includes(row.id));
      for (const row of newRows) {
        const data = JSON.parse(row.data);
        if (data.role !== "assistant") continue;
        const parts = db.prepare("SELECT data FROM part WHERE message_id = ? ORDER BY time_created, rowid").all(row.id)
          .map((part) => JSON.parse(part.data));
        const answer = parts.filter((part) => part.type === "text").map((part) => part.text).join("\n");
        if (answer.includes(marker) && data.time?.completed) return {
          source: path, session, message_id: row.id, model: `${data.providerID}/${data.modelID}`,
          answer, completed_at: data.time.completed, record: data,
        };
      }
    } finally { db.close(); }
    return null;
  }
  const rows = parseLines(readFileSync(path).subarray(boundary.bytes).toString("utf8"));
  let model = null, answer = "", completed = null, ownUser = false;
  for (const row of rows) {
    if (provider === "claude") {
      if (row.type === "user" && text(row.message?.content).includes(marker)) ownUser = true;
      if (ownUser && row.type === "assistant" && text(row.message?.content).includes(marker)) return {
        source: path, session, model: row.message.model, answer: text(row.message.content),
        completed_at: row.timestamp, record: row,
      };
    } else if (provider === "codex") {
      if (row.type === "turn_context") model = row.payload?.model;
      if (row.type === "response_item" && row.payload?.role === "user" && text(row.payload.content).includes(marker)) ownUser = true;
      if (ownUser && row.type === "response_item" && row.payload?.role === "assistant") answer += text(row.payload.content);
      if (ownUser && row.type === "event_msg" && ["task_complete", "turn_complete"].includes(row.payload?.type)) completed = row.timestamp;
    } else if (provider === "grok") {
      const update = row.params?.update;
      if (update?.sessionUpdate === "user_message_chunk" && text(update.content).includes(marker)) {
        ownUser = true;
        model = update._meta?.modelId;
      }
      if (ownUser && update?.sessionUpdate === "agent_message_chunk") answer += text(update.content);
      if (ownUser && update?.sessionUpdate === "turn_completed") completed = row.params?._meta?.agentTimestampMs ?? true;
    }
  }
  return ownUser && answer.includes(marker) && model && completed
    ? { source: path, session, model, answer, completed_at: completed } : null;
}
