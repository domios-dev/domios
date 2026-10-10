export interface DsTasksBlockItem {
  id: string;
  key: string;
  status: string;
  name: string;
}

export interface DsTasksBlock {
  planId: string;
  planTitle: string;
  filter: string[];
  items: DsTasksBlockItem[];
}

export type DsTaskGroupKey = "in_progress" | "in_review" | "blocked" | "todo" | "done" | "cancelled";

export const DS_TASK_GROUP_ALIASES: Readonly<Record<string, DsTaskGroupKey>> = {
  in_progress: "in_progress",
  "in-progress": "in_progress",
  in_review: "in_review",
  "in-review": "in_review",
  blocked: "blocked",
  todo: "todo",
  pending: "todo",
  paused: "todo",
  done: "done",
};

export const DS_PLAN_ID_RE = /^pl_[A-Za-z0-9_-]+$/;
export const DS_TASK_ID_RE = /^task_[A-Za-z0-9_-]+$/;
export const DS_TASK_KEY_RE = /^[A-Za-z][A-Za-z0-9]*-\d+$/;

export function isDsTaskRef(word: string): boolean {
  return DS_TASK_ID_RE.test(word) || DS_TASK_KEY_RE.test(word);
}

export interface ParsedDsTasksMessage {
  before: string;
  block: DsTasksBlock;
  after: string;
}

const BLOCK_RE = /<ds-tasks plan="([^"\s]+)"(?: filter="([^"]*)")?>\r?\n([\s\S]*?)\r?\n?<\/ds-tasks>/;
const TITLE_RE = /^Plan: (.*)$/;
const ITEM_RE = /^- (\S+) (\S+) \[([a-z_]+)\] ?(.*)$/;

function oneLine(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

/** The compact text the agent receives; the chat renders the rich block from this same text. */
export function formatDsTasksBlock(block: DsTasksBlock): string {
  const filter = block.filter.length > 0 ? ` filter="${block.filter.join(",")}"` : "";
  const lines = [
    `<ds-tasks plan="${block.planId}"${filter}>`,
    `Plan: ${oneLine(block.planTitle) || block.planId}`,
    ...block.items.map((item) => `- ${item.key} ${item.id} [${item.status}] ${oneLine(item.name)}`.trimEnd()),
    "</ds-tasks>",
  ];
  return lines.join("\n");
}

export function parseDsTasksMessage(content: string): ParsedDsTasksMessage | null {
  const match = BLOCK_RE.exec(content);
  if (!match) return null;
  const [whole, planId, filter, body] = match;
  let planTitle = planId;
  const items: DsTasksBlockItem[] = [];
  for (const line of body.split(/\r?\n/)) {
    const title = TITLE_RE.exec(line);
    if (title) {
      planTitle = title[1].trim() || planId;
      continue;
    }
    const item = ITEM_RE.exec(line);
    if (item) items.push({ key: item[1], id: item[2], status: item[3], name: item[4].trim() });
  }
  return {
    before: content.slice(0, match.index).trim(),
    block: {
      planId,
      planTitle,
      filter: filter ? filter.split(",").filter(Boolean) : [],
      items,
    },
    after: content.slice(match.index + whole.length).trim(),
  };
}
