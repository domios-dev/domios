// Middle-truncate path slugs so the tail (branch name) stays readable in chips.
export function middleTruncateSlug(path: string, maxLen = 28): string {
  const text = path.trim();
  if (text.length <= maxLen) return text;
  const slash = text.lastIndexOf("/");
  if (slash < 0) {
    const tail = Math.max(6, Math.floor(maxLen * 0.45));
    return `${text.slice(0, maxLen - tail - 1)}…${text.slice(-tail)}`;
  }
  const prefix = text.slice(0, slash + 1);
  const slug = text.slice(slash + 1);
  const slugBudget = maxLen - prefix.length - 1;
  if (slugBudget >= slug.length) return text;
  const keep = Math.max(6, Math.min(slug.length, slugBudget));
  return `${prefix}…${slug.slice(-keep)}`;
}
