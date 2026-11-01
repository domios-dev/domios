export const CHOICE_MIN_OPTIONS = 2;
export const CHOICE_MAX_OPTIONS = 8;
export const CHOICE_MAX_LABEL = 80;

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
