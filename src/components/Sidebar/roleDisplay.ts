const ACRONYMS: Record<string, string> = { qa: "QA" };

/** Title-cases an uppercase kind token; a Team-registered label already carries its declared case. */
export function roleDisplayWord(word: string): string {
  if (word !== word.toUpperCase()) return word;
  const acronym = ACRONYMS[word.toLowerCase()];
  if (acronym) return acronym;
  return word.charAt(0).toUpperCase() + word.slice(1).toLowerCase();
}
