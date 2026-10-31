export interface MentionScope {
  // The composer owns scope; active UI selection and machine-global defaults cannot widen it.
  tabId: string;
  workspaceId: string | null;
  projectPath: string | null;
  cwd?: string | null;
  teamId: string | null;
}
export interface MentionContext { scope: MentionScope; signal: AbortSignal }
export interface ComposerMention {
  id: string;
  kind?: "tab" | "task" | "plan" | "blueprint" | "file";
  providerId?: string | null;
  status?: string;
  label: string;
  description: string;
  insertText: string;
  group: string;
  match: number;
  exact?: boolean;
  proximity?: number;
  agentOrder?: number;
  statusOrder?: number;
}
export interface MentionSource {
  id: string;
  group: string;
  search(query: string, ctx: MentionContext): Promise<ComposerMention[]>;
}
export interface MentionQuery { start: number; end: number; partial: string }
export function activeMentionQuery(draft: string, caret = draft.length, selectionEnd = caret): MentionQuery | null {
  if (caret !== selectionEnd || caret < 0 || caret > draft.length) return null;
  const match = /(^|\s)@([\w-]*)$/.exec(draft.slice(0, caret));
  if (!match) return null;
  const tail = /^[\w-]*/.exec(draft.slice(caret))?.[0] ?? "";
  return { start: match.index + match[1].length, end: caret + tail.length, partial: match[2] };
}
export function applyComposerMention(draft: string, query: MentionQuery, mention: Pick<ComposerMention, "insertText">): string {
  return `${draft.slice(0, query.start)}${mention.insertText} ${draft.slice(query.end)}`;
}
export function mentionMatch(query: string, values: readonly string[]): number {
  const needle = query.toLowerCase();
  if (!needle) return 1;
  return Math.min(...values.map((value) => {
    const text = value.toLowerCase();
    if (text === needle) return 0;
    if (text.startsWith(needle)) return 1;
    let cursor = 0;
    for (const char of text) if (char === needle[cursor]) cursor++;
    return cursor === needle.length ? 2 : Infinity;
  }));
}
export function compareMentions(a: ComposerMention, b: ComposerMention): number {
  return Number(!!b.exact) - Number(!!a.exact)
    || (a.proximity ?? 0) - (b.proximity ?? 0)
    || (a.agentOrder ?? 0) - (b.agentOrder ?? 0)
    || (a.statusOrder ?? 0) - (b.statusOrder ?? 0)
    || a.match - b.match
    || a.label.localeCompare(b.label, undefined, { numeric: true });
}
export function createMentionRegistry(sources: readonly MentionSource[], lifecycle: { subscribe?: (changed: () => void) => () => void; close?: () => void } = {}) {
  if (new Set(sources.map((source) => source.id)).size !== sources.length) throw new Error("Duplicate mention source");
  return {
    subscribe: lifecycle.subscribe,
    close: lifecycle.close,
    async search(query: string, ctx: MentionContext): Promise<ComposerMention[]> {
      ctx.signal.throwIfAborted();
      const groups = await Promise.all(sources.map(async (source) => {
        try { return (await source.search(query, ctx)).filter((row) => Number.isFinite(row.match)).sort(compareMentions).slice(0, 50); }
        catch (error) { if (ctx.signal.aborted) throw error; return []; }
      }));
      ctx.signal.throwIfAborted();
      return groups.sort((a, b) => Number(!!b[0]?.exact) - Number(!!a[0]?.exact)).flat();
    },
  };
}
export interface MentionDismissal { start: number; draft: string; caret: number }
export function mentionDismissed(dismissed: MentionDismissal | null, draft: string, caret: number, query: MentionQuery): boolean {
  if (!dismissed || query.start !== dismissed.start) return false;
  if (draft.length < dismissed.draft.length && caret <= dismissed.caret) return false;
  return true;
}
export const MENTION_PICKER_LAYOUT = { placement: "top-start", width: "var(--mention-picker-width)", rowHeight: 44, visibleRows: 6 } as const;
