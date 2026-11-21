import type { ComposerMention, MentionQuery } from "../composerMentions";

export const DICTATION_ACTION = "action:dictation";

export function composerActionQuery(input: string, start: number, end: number): MentionQuery | null {
  if (start !== end) return null;
  const match = /@(dict[a-z]*)$/i.exec(input.slice(0, start));
  if (!match || !composerActions(match[1]).length) return null;
  return { start: match.index, end: start, partial: match[1] };
}

export function composerActions(query: string): ComposerMention[] {
  const label = "Dictation";
  if (!label.toLowerCase().startsWith(query.toLowerCase())) return [];
  return [{ id: DICTATION_ACTION, kind: "action", label, description: "Speak into this draft",
    insertText: "", group: "Actions", exact: query.toLowerCase() === label.toLowerCase(), titlePrefix: true, match: 0 }];
}
