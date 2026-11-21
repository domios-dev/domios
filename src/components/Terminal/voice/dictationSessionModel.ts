import type { DictationUpdate } from "../../../../gen/DictationUpdate";
export type { DictationUpdate } from "../../../../gen/DictationUpdate";

export interface DictationEdit {
  start: number;
  end: number;
  text: string;
}

export interface DictationSelection {
  start: number;
  end: number;
  inputType: string;
}

function replace(draft: string, { start, end, text }: DictationEdit): string {
  return draft.slice(0, start) + text + draft.slice(end);
}

export function changedDictationRange(before: string, after: string, selection?: DictationSelection): DictationEdit {
  if (selection) {
    let { start, end } = selection;
    const removed = before.length - after.length;
    if (start === end && removed > 0) {
      if (selection.inputType.endsWith("Backward")) start = Math.max(0, start - removed);
      if (selection.inputType.endsWith("Forward")) end = Math.min(before.length, end + removed);
    }
    const nextEnd = after.length - (before.length - end);
    if (nextEnd >= start && before.slice(0, start) === after.slice(0, start)
      && before.slice(end) === after.slice(nextEnd)) {
      return { start, end, text: after.slice(start, nextEnd) };
    }
  }
  let start = 0;
  let end = before.length;
  let nextEnd = after.length;
  while (start < end && start < nextEnd && before[start] === after[start]) start++;
  while (end > start && nextEnd > start && before[end - 1] === after[nextEnd - 1]) {
    end--;
    nextEnd--;
  }
  return { start, end, text: after.slice(start, nextEnd) };
}

const WORD_CHAR = /[\p{L}\p{N}]/u;
const DIGIT_CHAR = /\p{N}/u;
const AFTER_PUNCTUATION = /[.!?,;:…)]/;

function needsSpaceBetween(leftText: string, rightChar?: string): boolean {
  if (!leftText || !rightChar || !WORD_CHAR.test(rightChar)) return false;
  const left = leftText[leftText.length - 1];
  if (WORD_CHAR.test(left)) return true;
  if (!AFTER_PUNCTUATION.test(left)) return false;
  const prev = leftText[leftText.length - 2];
  if ((left === "." || left === ",") && prev && DIGIT_CHAR.test(prev) && DIGIT_CHAR.test(rightChar)) return false;
  return true;
}

export class DictationSessionModel {
  private sequence = 0;
  private closed = false;
  private visible: string;
  private original: string;
  private provisional = false;
  private hasCommitted = false;

  constructor(
    private readonly session: string,
    draft: string,
    private start: number,
    private end: number,
  ) {
    this.visible = draft;
    this.original = draft.slice(start, end);
  }

  get text() { return this.visible; }
  get durable() {
    return this.provisional ? replace(this.visible, { start: this.start, end: this.end, text: this.restoreText() }) : this.visible;
  }

  private restoreText(): string {
    const before = this.visible.slice(Math.max(0, this.start - 2), this.start);
    const separator = this.hasCommitted
      && needsSpaceBetween(before, this.original[0] ?? this.visible[this.end]) ? " " : "";
    return separator + this.original;
  }

  update(update: DictationUpdate): DictationEdit | null | undefined {
    if (this.closed || update.session !== this.session || update.sequence !== this.sequence + 1) return null;
    this.sequence = update.sequence;
    this.closed = update.final;
    const committed = update.final ? [update.committed, update.provisional].filter(Boolean).join(" ") : update.committed;
    const pending = update.final ? "" : update.provisional;
    if (!committed && !pending && !this.provisional) return;
    const before = this.visible.slice(Math.max(0, this.start - 2), this.start);
    const after = this.visible[this.end];
    const fixed = committed && needsSpaceBetween(before, committed[0]) ? " " + committed : committed;
    const beforePreview = fixed ? (before + fixed).slice(-2) : before;
    const preview = pending && needsSpaceBetween(beforePreview, pending[0]) ? " " + pending : pending;
    const phrase = fixed + preview;
    const tailSource = phrase ? (before + phrase).slice(-2) : (this.hasCommitted ? before : "");
    const spacer = needsSpaceBetween(tailSource, after) ? " " : "";
    const text = phrase || this.hasCommitted ? phrase + spacer : this.original;
    const edit = { start: this.start, end: this.end, text };
    const changed = this.visible.slice(edit.start, edit.end) !== edit.text;
    if (changed) this.visible = replace(this.visible, edit);
    if (fixed) {
      this.start += fixed.length;
      this.hasCommitted = true;
    }
    this.end = edit.start + edit.text.length;
    this.provisional = !update.final && (!!preview || this.hasCommitted && (!!this.original || !!spacer));
    return changed || fixed ? edit : undefined;
  }

  edit(next: string, selection?: DictationSelection): void {
    if (next === this.visible) return;
    const edit = changedDictationRange(this.visible, next, selection);
    if (edit.end <= this.start) {
      const shift = edit.text.length - (edit.end - edit.start);
      this.start += shift;
      this.end += shift;
    } else if (edit.start < this.end) {
      if (this.provisional) {
        const replacedOwnedSpan = edit.start <= this.start && edit.end >= this.end && edit.start !== edit.end;
        const restore = (replacedOwnedSpan ? "" : this.restoreText()) + edit.text;
        const start = Math.min(edit.start, this.start);
        const end = Math.max(edit.end, this.end);
        this.visible = replace(this.visible, { start, end, text: restore });
        this.start = start + restore.length;
        this.end = this.start;
        this.original = "";
        this.hasCommitted = false;
        this.provisional = false;
        return;
      }
      this.start = edit.start + edit.text.length;
      this.end = this.start;
      this.original = "";
    }
    this.visible = next;
  }

  cancel(): DictationEdit | undefined {
    if (this.closed) return;
    this.closed = true;
    if (!this.provisional) return;
    const edit = { start: this.start, end: this.end, text: this.restoreText() };
    this.visible = replace(this.visible, edit);
    this.provisional = false;
    return edit;
  }
}
