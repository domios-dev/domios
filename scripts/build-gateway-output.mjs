// Caller: build.sh
import fs from 'node:fs';
import readline from 'node:readline';
import { once } from 'node:events';
import { pathToFileURL } from 'node:url';

export function fingerprintSummary(text) {
  const counts = new Map();
  let lines = 0;
  for (const line of text.split('\n')) {
    if (!line.includes('cargo::core::compiler::fingerprint')) continue;
    lines++;
    const reason = line.match(/fingerprint:\s+(?:dirty|err):\s*([A-Za-z]+)/)?.[1];
    if (reason) counts.set(reason, (counts.get(reason) ?? 0) + 1);
  }
  return { lines, dirty_reasons: [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).slice(0, 5) };
}

export function testSummary(text) {
  const summaries = text.split('\n').filter(line => /^\s*Summary\b|^test result:/.test(line));
  return { runs: summaries.length, last: summaries.at(-1)?.slice(0, 240) ?? 'runner produced no summary' };
}

export function testExecutionResult(text, {
  status = 0, filter = '', noTestsMode = '', requireExecution = false,
} = {}) {
  if (status !== 0) return { status, error: null };
  if (!requireExecution && (!filter || ['pass', 'warn'].includes(noTestsMode))) return { status: 0, error: null };
  let stressDetails = false;
  let stressPassed = false;
  for (const line of text.replaceAll(/\x1b\[[0-9;]*m/g, '').split(/\r?\n/)) {
    const libtest = line.match(/^\s*test result: [A-Za-z]+\. (\d+) passed;/);
    const tests = line.match(/^\s*(Summary \[[^\]]*\]\s+|Stress test \[[^\]]*\] iteration [^:]+: )\d+ tests? run: (\d+) passed/);
    if (Number(libtest?.[1]) > 0 || Number(tests?.[2]) > 0) return { status: 0, error: null };
    if (tests && line.includes('Stress test')) stressDetails = true;
    const iterations = line.match(/^\s*Summary \[[^\]]*\]\s+\d+(?:\/\d+)? stress run iterations?: (\d+) passed/);
    if (Number(iterations?.[1]) > 0) stressPassed = true;
  }
  if (!requireExecution && !stressDetails && stressPassed) return { status: 0, error: null };
  return {
    status: 97,
    error: requireExecution
      ? 'test-one requires at least one passed test; zero executed tests or a missing runner summary cannot pass.'
      : `ZERO MATCHES: filter ${filter} selected no tests.`,
  };
}

export async function filterFingerprint(input, output, artifact) {
  let degraded = false;
  let artifactFailed = false;
  let outputFailed = false;
  output.on('error', () => { outputFailed = true; degraded = true; });
  const visible = async text => {
    if (outputFailed) return;
    try { if (!output.write(text)) await once(output, 'drain'); }
    catch { outputFailed = true; degraded = true; }
  };
  for await (const line of readline.createInterface({ input, crlfDelay: Infinity })) {
    if (line.includes('cargo::core::compiler::fingerprint')) {
      if (!artifactFailed) {
        try { fs.appendFileSync(artifact, `${line}\n`); }
        catch { artifactFailed = true; degraded = true; await visible('WARNING: fingerprint artifact unavailable; trace discarded, command continues\n'); }
      }
    } else await visible(`${line}\n`);
  }
  return { degraded };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [mode, file, status] = process.argv.slice(2);
  if (mode === 'filter') process.exitCode = (await filterFingerprint(process.stdin, process.stdout, file)).degraded ? 1 : 0;
  else if (mode === 'fingerprint-summary') {
    const summary = fingerprintSummary(fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : '');
    console.log(`fingerprints: lines=${summary.lines} top-dirty-reasons=${JSON.stringify(summary.dirty_reasons)} artifact=${file}`);
  } else if (mode === 'test-summary') {
    const text = fs.readFileSync(file, 'utf8');
    const summary = testSummary(text);
    console.log(`tests: runs=${summary.runs} exit=${status} ${summary.last} artifact=${file}`);
    if (Number(status) !== 0) console.log(text.split('\n').filter(Boolean).slice(-25).join('\n'));
  } else if (mode === 'test-proof') {
    const [filter, noTestsMode] = process.argv.slice(4);
    const result = testExecutionResult(fs.readFileSync(file, 'utf8'), {
      filter, noTestsMode, requireExecution: process.env.TEST_REQUIRE_EXECUTION === '1',
    });
    process.exitCode = result.status;
    if (result.error) console.error(`error: ${result.error}`);
  } else throw new Error(`Unknown gateway output operation: ${mode}`);
}
