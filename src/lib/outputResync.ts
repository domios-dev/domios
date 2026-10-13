export function planOutputResync(offset: number | undefined, resumeFrom: number): {
  offset: number;
  reportGap: boolean;
} {
  const previous = offset ?? 0;
  return { offset: Math.max(previous, resumeFrom), reportGap: resumeFrom > previous };
}
