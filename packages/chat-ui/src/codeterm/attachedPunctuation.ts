const ATTACHED = /^[:;,.!?)\]}…]+/;

/** Punctuation that must stay on the line of the chip before it. */
export function splitAttachedPunctuation(text: string): [attached: string, rest: string] {
  const attached = ATTACHED.exec(text)?.[0] ?? "";
  return [attached, text.slice(attached.length)];
}
