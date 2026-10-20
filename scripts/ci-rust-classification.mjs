// Caller: scripts ci-rust-archives.mjs, ci-rust-tests.mjs, tests/ci-rust-tests.test.mjs
export function classifyInventory(inventory, sources) {
  const rows = [];
  for (const suite of Object.values(inventory['rust-suites'])) {
    if (suite.status !== 'listed') throw new Error(`Unlisted binary: ${suite['binary-id']}`);
    const definitions = sources[suite['binary-id']];
    if (!definitions) throw new Error(`No source root: ${suite['binary-id']}`);
    for (const [name, test] of Object.entries(suite.testcases)) {
      const matches = definitions.filter((entry) => entry.scope ? name.startsWith(entry.name) : name === entry.name || name.startsWith(`${entry.name}::`));
      if (!matches.length) throw new Error(`Unclassified test: ${suite['binary-id']} ${name}`);
      const reason = matches.find((entry) => entry.ignore_reason)?.ignore_reason;
      if (test.ignored && !reason) throw new Error(`Ignored test lacks a source-authored reason: ${suite['binary-id']} ${name}`);
      rows.push({ id: `${suite['binary-id']}::${name}`, binary: suite['binary-name'], package: suite['package-name'], name, ignored: test.ignored, reason: test.ignored ? reason : undefined, platform: matches.some((entry) => entry.platform) });
    }
  }
  return rows;
}

