export const ocHost = (over: Record<string, unknown>) => ({
  manifestJson: () => JSON.stringify({
    models: [
      { id: "seed-opencode", displayName: "Seed OpenCode", description: "Manifest fallback" },
    ],
  }),
  homeDir: () => "/tmp",
  platform: () => "linux",
  expandHome: (v: string) => v,
  envGet: () => null,
  exec: () => JSON.stringify({ code: 1, stdout: "", stderr: "not available" }),
  unixNowMs: () => 1_000_000,
  fetch: () => JSON.stringify({ error: "no network in tests" }),
  log: () => {},
  shell: { quoteFor: (v: unknown) => String(v), quote: (v: unknown) => String(v) },
  fs: { fileExists: () => false, readJson: () => null, readFile: () => null },
  ...over,
});
