export interface OptionGroup<T> {
  label: string;
  options: T[];
}

/** Buckets options by their declared `group`, preserving first-appearance order
 *  of sections and original order within each. Returns null when no option
 *  declares a group, so callers can keep rendering a flat list. */
export function groupOptionsByLabel<T extends { group?: string | null }>(
  options: T[],
): OptionGroup<T>[] | null {
  if (!options.some((option) => (option.group ?? "").trim().length > 0)) return null;
  const order: string[] = [];
  const buckets = new Map<string, T[]>();
  for (const option of options) {
    const label = (option.group ?? "").trim();
    let bucket = buckets.get(label);
    if (!bucket) {
      bucket = [];
      buckets.set(label, bucket);
      order.push(label);
    }
    bucket.push(option);
  }
  return order.map((label) => ({ label, options: buckets.get(label)! }));
}
