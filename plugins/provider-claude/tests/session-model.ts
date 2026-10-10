interface ModelDetector {
    detectSessionModel(cwd: string, sessionId: string): unknown;
    detectSessionReasoningEffort(cwd: string, sessionId: string): unknown;
}

export function runSessionModelTests(
  loadClaudeWithHost: (host: unknown) => ModelDetector,
  baseHost: (overrides: Record<string, unknown>) => unknown,
  eq: (actual: unknown, expected: unknown, name: string) => void,
) {
console.log("\n--- session model detection ---\n");
{
  const command = { type: "user", message: { content: "<local-command-stdout>Set effort level to medium (saved as your default for new sessions): Balanced approach with standard implementation and testing</local-command-stdout>" } };
  const prior = { type: "assistant", message: { metadata: { effort: "low" } } };
  const cases: Array<[unknown[], string]> = [
    [[prior, command], "medium"],
    [[prior, command, { type: "assistant", message: { metadata: { effort: "high" } } }], "high"],
    [[prior, { type: "user", message: { content: "Set effort level to medium (saved as your default for new sessions): ordinary user text" } }], "low"],
    [[prior, { ...command, type: "assistant" }], "low"],
  ];
  for (const [records, expected] of cases) {
    const p = loadClaudeWithHost(baseHost({ fs: { fileExists: () => true, readFileTail: () => records.map(record => JSON.stringify(record)).join("\n") } }));
    eq(p.detectSessionReasoningEffort("/fixture/project", "owned-session"), expected, "effort uses newest authoritative record and excludes ordinary wrapper text");
  }
}
{
  const transcript = [
    JSON.stringify({ type: "assistant", message: { model: "claude-opus-5-5" } }),
    JSON.stringify({ type: "user", message: {
      content: "<local-command-stdout>Set model to `Sonnet 5.5` for this session only</local-command-stdout>",
    } }),
  ].join("\n");
  const p = loadClaudeWithHost(baseHost({ fs: {
    fileExists: () => true,
    readFileTail: () => transcript,
  } }));
  eq(p.detectSessionModel("/fixture/project", "owned-session"), "Sonnet",
    "backtick command stdout wins over the previous assistant model");
}
{
  for (const record of [
    { type: "user", message: { content: "Set model to `Haiku 5.5` for this session only" } },
    { type: "assistant", message: { content: "<local-command-stdout>Set model to `Haiku 5.5` for this session only</local-command-stdout>" } },
  ]) {
    const transcript = [JSON.stringify({ type: "assistant", message: { model: "claude-opus-5-5" } }),
      JSON.stringify(record)].join("\n");
    const p = loadClaudeWithHost(baseHost({ fs: { fileExists: () => true, readFileTail: () => transcript } }));
    eq(p.detectSessionModel("/fixture/project", "owned-session"), "Opus",
      "ordinary message text cannot override the actual model");
  }
}
{
  const transcript = '{"type":"assistant","message":{"model":"claude-opus-5","content":[]}}';
  let fullReads = 0;
  let tailBytes = 0;
  const p = loadClaudeWithHost(baseHost({
    fs: {
      fileExists: () => true,
      readFile: () => { fullReads += 1; return transcript; },
      readFileTail: (_path: string, maxBytes: number) => {
        tailBytes = maxBytes;
        return transcript;
      },
      readJson: () => null,
      writeTempFile: () => "/tmp/ct-task",
      writeJson: () => true,
    },
  }));

  eq(
    p.detectSessionModel("/fixture/project", "session-model"),
    "Opus",
    "session model uses the exact session transcript",
  );
  eq(fullReads, 0, "session model avoids loading the full transcript");
  eq(tailBytes, 4 * 1024 * 1024, "session model reads a bounded 4 MiB transcript tail");
}

}
