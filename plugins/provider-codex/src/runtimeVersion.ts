export function runtimeBinaryVersion(userAgent: unknown): string | undefined {
  if (typeof userAgent !== "string") return undefined;
  return userAgent.match(/^[^\s/]+\/(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?=\s|$)/)?.[0].split("/")[1];
}

export function initializedResult(state: Record<string, unknown>) {
  const binaryVersion = typeof state.binaryVersion === "string" ? state.binaryVersion : undefined;
  return { kind: "initialized" as const, ...(binaryVersion ? { binaryVersion } : {}) };
}
