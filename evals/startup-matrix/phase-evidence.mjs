import { readFileSync, writeFileSync, statSync, existsSync } from 'node:fs';
import { phaseBreakdown, logMarks } from './evidence-model.mjs';

const [phasesFile, stateFile, destination, logFile, begin, end] = process.argv.slice(2);
if (!destination) throw Error('Usage: phase-evidence.mjs PHASES_JSON STATE_JSON OUT_JSON [RAW_LOG BEGIN_US END_US]');
if (existsSync(destination)) throw Error('Fresh phase artifact path required');
const read = file => JSON.parse(readFileSync(file, 'utf8').replace(/^\uFEFF/, ''));
const input = read(phasesFile);
if (logFile) {
  if (statSync(logFile).size > 16 * 1024 * 1024) throw Error('Raw phase log exceeded byte bound');
  input.marks = logMarks(readFileSync(logFile, 'utf8'), Number(begin), Number(end));
}
const rows = phaseBreakdown(input, read(stateFile));
const result = { schema: 1, scenario: 'phase-origins', status: rows.length && rows.every(row => !row.missing.length) ? 'captured' : 'incomplete', rows };
writeFileSync(destination, JSON.stringify(result, null, 2));
if (result.status === 'incomplete') process.exitCode = 1;
