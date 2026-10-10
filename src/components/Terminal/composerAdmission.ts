export function createComposerAdmission() {
  const pending = new Set<number>();
  return {
    claim(revision: number): (() => void) | null {
      if (pending.has(revision)) return null;
      pending.add(revision);
      return () => pending.delete(revision);
    },
  };
}
