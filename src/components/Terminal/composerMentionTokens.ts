import { referenceMarkers } from "../../../packages/chat-ui/src/codeterm/referenceMarkers";

export interface MentionToken {
  start: number;
  end: number;
  sourceStart: number;
  sourceEnd: number;
  marker: string;
  title: string;
  id: string;
}
export interface MentionDraft { canonical: string; text: string; tokens: MentionToken[] }
export interface TextRange { start: number; end: number }
type TokenLabel = Pick<MentionToken, "sourceStart" | "id" | "title">;
interface MentionEdit { start: number; before: string; after: string; beforeLength: number; afterLength: number; beforeTokens: TokenLabel[]; afterTokens: TokenLabel[] }

export function projectMentionDraft(canonical: string, title: (id: string, sourceStart: number) => string): MentionDraft {
  let text = "", cursor = 0;
  const tokens: MentionToken[] = [];
  for (const marker of referenceMarkers(canonical)) {
    if (marker.kind !== "tab") continue;
    text += canonical.slice(cursor, marker.start);
    const label = title(marker.id, marker.start).replace(/[\r\n\t]/g, " ") || "Tab";
    const start = text.length;
    text += `@${label}`;
    tokens.push({ start, end: text.length, sourceStart: marker.start, sourceEnd: marker.end, marker: marker.text, title: label, id: marker.id });
    cursor = marker.end;
  }
  return { canonical, text: text + canonical.slice(cursor), tokens };
}

export function snapMentionSelection(tokens: readonly MentionToken[], start: number, end: number): TextRange {
  if (start === end) {
    const token = tokens.find(token => token.start < start && token.end > start);
    const edge = token ? (start - token.start < (token.end - token.start) / 2 ? token.start : token.end) : start;
    return { start: edge, end: edge };
  }
  for (const token of tokens) {
    if (start < token.end && end > token.start) { start = Math.min(start, token.start); end = Math.max(end, token.end); }
  }
  return { start, end };
}

export function mentionDeletionRange(tokens: readonly MentionToken[], start: number, end: number, direction: "Backspace" | "Delete", wordText?: string): TextRange | null {
  if (start !== end) return tokens.some(token => start < token.end && end > token.start) ? snapMentionSelection(tokens, start, end) : null;
  const token = tokens.find(token => direction === "Backspace"
    ? token.end === start || wordText !== undefined && token.end < start && /^\s+$/.test(wordText.slice(token.end, start))
    : token.start === start || wordText !== undefined && token.start > start && /^\s+$/.test(wordText.slice(start, token.start)));
  return token ? { start: token.start, end: token.end } : null;
}

export function mentionArrowTarget(tokens: readonly MentionToken[], caret: number, direction: "ArrowLeft" | "ArrowRight"): number | null {
  const token = tokens.find(token => direction === "ArrowLeft" ? token.start < caret && caret <= token.end : token.start <= caret && caret < token.end);
  return token ? direction === "ArrowLeft" ? token.start : token.end : null;
}

function sourceOffset(draft: MentionDraft, position: number): number {
  let offset = position;
  for (const token of draft.tokens) {
    if (position <= token.start) break;
    if (position < token.end) return token.sourceStart;
    offset += token.marker.length - (token.end - token.start);
  }
  return offset;
}

export function canonicalMentionSlice(draft: MentionDraft, start: number, end: number): string {
  return draft.canonical.slice(sourceOffset(draft, start), sourceOffset(draft, end));
}

export function replaceMentionRange(draft: MentionDraft, start: number, end: number, canonical: string): string {
  return draft.canonical.slice(0, sourceOffset(draft, start)) + canonical + draft.canonical.slice(sourceOffset(draft, end));
}

export function normalizePastedMentions(text: string): string {
  return text.replace(/(?:@[^\s()]+\s+)?\(tab ([\w:./-]+)\)/g, (_, id: string) => `<domios type="tab" id="${id}" />`);
}

function changedRange(before: string, after: string) {
  let start = 0, end = before.length, nextEnd = after.length;
  while (start < end && start < nextEnd && before[start] === after[start]) start++;
  while (end > start && nextEnd > start && before[end - 1] === after[nextEnd - 1]) { end--; nextEnd--; }
  return { start, end, nextEnd };
}

export class MentionTokenModel {
  private history: MentionEdit[] = [];
  private cursor = 0;
  current: MentionDraft;
  pending: MentionDraft | null = null;

  constructor(canonical: string, private resolve: (id: string) => string) {
    this.current = this.project(canonical);
  }

  project(canonical: string, labels: readonly TokenLabel[] = []): MentionDraft {
    const previous = this.current;
    const range = changedRange(previous?.canonical ?? "", canonical);
    const explicitLabels = new Map(labels.map(label => [label.sourceStart, label]));
    const priorLabels = new Map(previous?.tokens.map(token => [token.sourceStart, token]));
    return projectMentionDraft(canonical, (id, sourceStart) => {
      const explicit = explicitLabels.get(sourceStart);
      if (explicit?.id === id) return explicit.title;
      const oldStart = sourceStart < range.start ? sourceStart : sourceStart >= range.nextEnd ? sourceStart + range.end - range.nextEnd : -1;
      const prior = priorLabels.get(oldStart);
      return prior?.id === id ? prior.title : this.resolve(id);
    });
  }

  sync(canonical: string): MentionDraft {
    if (canonical !== this.current.canonical) this.record(this.project(canonical));
    return this.current;
  }

  prepare(start: number, end: number, canonical: string, label?: { id: string; title: string }): MentionDraft {
    const inserted = label ? referenceMarkers(canonical).filter(marker => marker.kind === "tab" && marker.id === label.id)
      .map(marker => ({ ...label, sourceStart: sourceOffset(this.current, start) + marker.start })) : [];
    this.pending = this.project(replaceMentionRange(this.current, start, end, canonical), inserted);
    return this.pending;
  }

  change(text: string, inputType = ""): MentionDraft {
    if (text === this.current.text && !this.pending) return this.current;
    if (this.pending?.text === text) { const next = this.pending; this.pending = null; return this.record(next); }
    this.pending = null;
    if (inputType === "historyUndo" || inputType === "historyRedo") {
      const step = inputType === "historyUndo" ? -1 : 1;
      let canonical = this.current.canonical;
      let labels: TokenLabel[] = this.current.tokens;
      for (let index = this.cursor; index + step >= 0 && index + step <= this.history.length; index += step) {
        const edit = this.history[step === -1 ? index - 1 : index];
        const removed = step === -1 ? edit.after : edit.before;
        const restored = step === -1 ? edit.before : edit.after;
        const removedTokens = step === -1 ? edit.afterTokens : edit.beforeTokens;
        const restoredTokens = step === -1 ? edit.beforeTokens : edit.afterTokens;
        const removedStarts = new Set(removedTokens.map(token => token.sourceStart));
        labels = labels.filter(token => !removedStarts.has(token.sourceStart)).map(token => token.sourceStart >= edit.start + removed.length
          ? { ...token, sourceStart: token.sourceStart + restored.length - removed.length } : token).concat(restoredTokens);
        canonical = canonical.slice(0, edit.start) + restored + canonical.slice(edit.start + removed.length);
        if (text.length !== (step === -1 ? edit.beforeLength : edit.afterLength)) continue;
        const next = this.project(canonical, labels);
        if (next.text === text) { this.cursor = index + step; return this.current = next; }
      }
    }
    const before = this.current;
    let start = 0, end = before.text.length, nextEnd = text.length;
    while (start < end && start < nextEnd && before.text[start] === text[start]) start++;
    while (end > start && nextEnd > start && before.text[end - 1] === text[nextEnd - 1]) { end--; nextEnd--; }
    const range = snapMentionSelection(before.tokens, start, end);
    const replacement = before.text.slice(range.start, start) + text.slice(start, nextEnd) + before.text.slice(end, range.end);
    return this.record(this.project(replaceMentionRange(before, range.start, range.end, replacement)));
  }

  private record(next: MentionDraft): MentionDraft {
    const before = this.current.canonical, after = next.canonical;
    if (before === after && this.current.text === next.text) return this.current;
    const { start, end, nextEnd } = changedRange(before, after);
    const oldLabels = new Map(this.current.tokens.map(token => [token.sourceStart, token]));
    const newLabels = new Map(next.tokens.map(token => [token.sourceStart, token]));
    const changedLabel = (token: MentionToken, other: MentionToken | undefined) => other?.id === token.id && other.title !== token.title;
    const label = ({ id, title, sourceStart }: MentionToken): TokenLabel => ({ id, title, sourceStart });
    const beforeTokens = this.current.tokens.filter(token => token.sourceStart < end && token.sourceEnd > start || changedLabel(token, newLabels.get(token.sourceStart))).map(label);
    const afterTokens = next.tokens.filter(token => token.sourceStart < nextEnd && token.sourceEnd > start || changedLabel(token, oldLabels.get(token.sourceStart))).map(label);
    this.history.splice(this.cursor);
    this.history.push({ start, before: before.slice(start, end), after: after.slice(start, nextEnd), beforeLength: this.current.text.length, afterLength: next.text.length, beforeTokens, afterTokens });
    this.cursor = this.history.length;
    this.current = next;
    return next;
  }
}
