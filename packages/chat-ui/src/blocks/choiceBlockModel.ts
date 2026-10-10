export const CHOICE_MIN_OPTIONS = 2;
export const CHOICE_MAX_OPTIONS = 8;
export const CHOICE_MAX_LABEL = 80;

/** `Label one | Label two`; `\|` keeps a literal bar. Blank, duplicate, too long or too few labels leave the line as text. */
export function parseChoiceOptions(argument: string): string[] | null {
  const labels = argument.split(/(?<!\\)\|/).map((part) => part.replace(/\\\|/g, "|").replace(/\s+/g, " ").trim());
  if (labels.length < CHOICE_MIN_OPTIONS || labels.length > CHOICE_MAX_OPTIONS) return null;
  if (labels.some((label) => !label || label.length > CHOICE_MAX_LABEL)) return null;
  const seen = new Set(labels.map(normalizeAnswer));
  return seen.size === labels.length ? labels : null;
}

export function normalizeAnswer(text: string): string {
  return text.replace(/\s+/g, " ").trim().toLowerCase();
}

/** What the host knows about the message a choice sits in: the user's next reply in that chat, if any. */
export interface ChoiceBlockState {
  reply: string | null;
}

export type ChoiceOutcome =
  | { kind: "open" }
  | { kind: "sending"; label: string }
  | { kind: "chosen"; label: string }
  | { kind: "answered"; reply: string }
  | { kind: "unavailable" };

/** The transcript is the truth: a reply that matches an option is that choice; any other reply closes the question. */
export function choiceOutcome(options: readonly string[], state: ChoiceBlockState | null, sending: string | null): ChoiceOutcome {
  if (!state) return { kind: "unavailable" };
  const reply = state.reply?.trim();
  if (reply) {
    const chosen = options.find((option) => normalizeAnswer(option) === normalizeAnswer(reply));
    return chosen ? { kind: "chosen", label: chosen } : { kind: "answered", reply };
  }
  return sending ? { kind: "sending", label: sending } : { kind: "open" };
}
