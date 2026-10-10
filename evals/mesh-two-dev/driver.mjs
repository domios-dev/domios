import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { matrix, markdown, applyVerdict } from './matrix.mjs';
import { jsonLines, proveRecord } from './records.mjs';
import { summarizeRecords, proveMediumChat } from './provider-records.mjs';
import { evidencePath, requireEvidence, appendVerdict } from './evidence.mjs';
const [mode, rootArg, input] = process.argv.slice(2);
if (!['prepare', 'record', 'render'].includes(mode) || !rootArg) throw new Error('Usage: driver.mjs prepare|record|render RUN_DIR [VERDICT_JSON]');
const root = resolve(rootArg);
await mkdir(root, { recursive: true });
let rows = matrix();
if (mode !== 'prepare') {
  const journal = await readFile(evidencePath(root, 'verdicts.jsonl'), 'utf8').catch(error => {
    if (error.code === 'ENOENT') return '';
    throw error;
  });
  for (const verdict of jsonLines(journal)) applyVerdict(rows, verdict);
}
if (mode === 'record') {
  const verdict = JSON.parse(await readFile(input, 'utf8'));
  await requireEvidence(root, verdict.evidence);
  if (verdict.providerProof) {
    const proof = verdict.providerProof;
    if (!verdict.evidence.includes(proof.sourcePath)) throw new Error('Raw provider record must be attached');
    const raw = await readFile(evidencePath(root, proof.sourcePath), 'utf8');
    if (verdict.status === 'PASS' && verdict.check === 3) {
      if (proof.provider !== verdict.provider) throw new Error('Provider proof does not match row');
      const summary = summarizeRecords(jsonLines(raw), proof, proof.schema);
      verdict.providerProof = { ...proof, ...proveMediumChat(summary, proof.expectedModels) };
    } else verdict.providerProof = proveRecord(jsonLines(raw), proof);
  }
  applyVerdict(rows, verdict);
  await appendVerdict(root, verdict);
}
if (mode === 'prepare') {
  await writeFile(evidencePath(root, 'results.json'), JSON.stringify(rows, null, 2), { flag: 'wx' });
} else await writeFile(evidencePath(root, 'results.json'), JSON.stringify(rows, null, 2));
await writeFile(evidencePath(root, 'results.md'), markdown(rows));
console.log(`${rows.filter(row => row.status === 'PASS').length} PASS; ${rows.filter(row => row.status === 'FAIL').length} FAIL; ${rows.filter(row => row.status === 'NOT_RUN').length} NOT_RUN`);
