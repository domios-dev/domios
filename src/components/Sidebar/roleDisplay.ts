const ACRONYMS: Record<string, string> = { qa: "QA" };

export function roleDisplayWord(word: string): string {
  const acronym = ACRONYMS[word.toLowerCase()];
  if (acronym) return acronym;
  return word.charAt(0).toUpperCase() + word.slice(1).toLowerCase();
}
