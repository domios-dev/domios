import { mkdir, writeFile, readFile, appendFile } from 'node:fs/promises';
import { resolve, relative, isAbsolute } from 'node:path';
import { createHash } from 'node:crypto';
export function evidencePath(root, name) {
  const path = resolve(root, name);
  const rel = relative(resolve(root), path);
  if (!rel || rel.startsWith('..') || isAbsolute(rel)) throw new Error('Evidence must stay in run directory');
  return path;
}
export async function writeEvidence(root, name, body) {
  const path = evidencePath(root, name);
  await mkdir(resolve(root), { recursive: true });
  await writeFile(path, body, { flag: 'wx' });
  return { path: name, sha256: createHash('sha256').update(body).digest('hex') };
}
export async function requireEvidence(root, names) {
  if (!names.length) throw new Error('Evidence required');
  return Promise.all(names.map(async name => {
    const body = await readFile(evidencePath(root, name));
    if (!body.length) throw new Error(`Empty evidence: ${name}`);
    return { path: name, sha256: createHash('sha256').update(body).digest('hex') };
  }));
}
export async function appendVerdict(root, verdict) {
  await requireEvidence(root, verdict.evidence);
  await appendFile(evidencePath(root, 'verdicts.jsonl'), JSON.stringify(verdict) + '\n');
}
