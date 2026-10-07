

export function decision(state, key, hash) {
  if (!key || state.active || state.claimed.includes(key) || (hash && state.lastHash === hash)) return 'waiting';
  return 'ready';
}

export function commands(surface) {
  const common = [['npx', 'tsc', '--noEmit']];
  if (surface === 'config' || surface === 'defaults') return [...common, ['node', 'scripts/run-ts-unit-tests.mjs', 'src/hooks/useConfig.test.ts']];
  if (surface === 'sdk') return [...common, ['npx', 'tsc', '--noEmit', '--strict', '--skipLibCheck', '--target', 'ES2021', '--module', 'ESNext', '--moduleResolution', 'bundler', 'packages/plugin-sdk/src/types.test.ts']];
  if (surface === 'automation') return [...common, ['node', 'scripts/run-ts-unit-tests.mjs', 'src/components/Automation/nodeDetailsModel.test.ts', 'src/components/Automation/CommandResultDetails.test.tsx', 'src/components/Automation/AutomationGraphView.test.tsx']];
  if (surface === 'shortcuts') return [...common, ['node', 'scripts/run-ts-unit-tests.mjs', 'src/help/shortcuts.test.ts']];
  throw new Error(`unknown surface: ${surface}`);
}

export function verdict(results) {
  return results.length > 0 && results.every(result => result.code === 0 && !result.signal && !result.error && !result.timeout && result.contained !== false) ? 'PASSED' : 'FAILED';
}

export function quote(value, platform = process.platform) {
  if (value.includes('\0')) throw new Error('NUL in argument');
  return platform === 'win32' ? `'${value.replaceAll("'", "''")}'` : `'${value.replaceAll("'", "'\\''")}'`;
}



export function missingTarget(command, exists) {
  return command.filter(arg => /\.test\.tsx?$/.test(arg)).find(arg => !exists(arg));
}
