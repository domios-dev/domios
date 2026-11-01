import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export const firebaseKeyPattern = new RegExp(['AI', 'za'].join(''), 'g');

export function checkFirebaseConfig(root) {
  const files = execFileSync('git', ['ls-files', '-z'], { cwd: root }).toString().split('\0').filter(Boolean);
  return files.filter(file => {
    firebaseKeyPattern.lastIndex = 0;
    return firebaseKeyPattern.test(readFileSync(resolve(root, file), 'utf8'));
  });
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const findings = checkFirebaseConfig(process.cwd());
  for (const file of findings) console.error(`Firebase web key literal: ${file}`);
  console.log(`Firebase config guard: ${findings.length} findings`);
  process.exitCode = findings.length ? 1 : 0;
}
