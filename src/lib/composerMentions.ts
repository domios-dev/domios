export interface ComposerMention {
  token: string;
  description: string;
}

export const COMPOSER_MENTIONS: readonly ComposerMention[] = [
  { token: "@ds-tasks", description: "Plan tasks, grouped by status" },
];

export interface MentionQuery {
  /** Offset of the `@` that starts the partial mention. */
  start: number;
  partial: string;
}

/** The partial mention being typed at the end of the draft, if any. */
export function activeMentionQuery(draft: string): MentionQuery | null {
  const match = /(^|\s)(@[\w-]*)$/.exec(draft);
  if (!match) return null;
  return { start: match.index + match[1].length, partial: match[2] };
}

export function filterComposerMentions(
  query: MentionQuery | null,
  mentions: readonly ComposerMention[] = COMPOSER_MENTIONS,
): ComposerMention[] {
  if (!query) return [];
  const needle = query.partial.toLowerCase();
  return mentions.filter((mention) => mention.token.startsWith(needle) && mention.token !== needle);
}

export function applyComposerMention(draft: string, query: MentionQuery, mention: ComposerMention): string {
  return `${draft.slice(0, query.start)}${mention.token} `;
}
