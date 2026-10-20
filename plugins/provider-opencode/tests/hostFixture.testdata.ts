export const ocHost = (over: Record<string, unknown>) => {
  const fixture = {
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
  };
  const fs = fixture.fs as Record<string, unknown>;
  const fetch = fixture.fetch as (opts: string) => string;
  return {
    ...fixture,
    fs: {
      readDir: () => [],
      sqliteQueryAsync: <R>(db: string, sql: string, params: unknown[], then: (rows: unknown[]) => R) =>
        then((fs.sqliteQuery as (db: string, sql: string, params: unknown[]) => unknown[])(db, sql, params)),
      ...fs,
    },
    fetch: Object.assign((opts: string) => fetch(opts), {
      async: <R>(opts: unknown, then: (result: unknown) => R) => then(JSON.parse(fetch(JSON.stringify(opts)))),
    }),
    execStart: () => JSON.stringify({ jobId: "fixture-job" }),
    awaitJob: <T>(_id: string, then: (result: unknown) => T) => then(JSON.parse((fixture.exec as () => string)())),
  };
};

type SqliteQuery = (db: string, sql: string, params: never[]) => unknown[];
export const withAsyncSqlite = <F extends { sqliteQuery: SqliteQuery }>(fs: F) => ({
  ...fs,
  sqliteQueryAsync: <R>(db: string, sql: string, params: never[], then: (rows: unknown[]) => R) =>
    then(fs.sqliteQuery(db, sql, params)),
});
