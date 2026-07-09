export function planFetchShowsData(loading: boolean, error: string | null, itemCount: number): boolean {
  if (itemCount > 0) return true;
  return !loading && !error;
}

export function planFetchShowsFullError(loading: boolean, error: string | null, itemCount: number): boolean {
  return !loading && !!error && itemCount === 0;
}
