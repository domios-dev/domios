export type GrammarLoadResult = "loaded" | "failed";
export type GrammarLoader = () => Promise<GrammarLoadResult>;

export function createGrammarLoader(load: () => Promise<unknown>, debug: (error: unknown) => void): GrammarLoader {
  let pending: Promise<GrammarLoadResult> | null = null;
  return () => {
    pending ??= Promise.resolve().then(load).then(() => "loaded" as const, (error: unknown) => {
      debug(error);
      return "failed" as const;
    });
    return pending;
  };
}
