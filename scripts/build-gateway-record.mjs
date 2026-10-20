// Caller: scripts/build-gateway-state.mjs, scripts/build-history.mjs
import fs from 'node:fs';
import path from 'node:path';

export function readJobJson(file, reader = fs.readFileSync) {
  try {
    const value = JSON.parse(reader(file, 'utf8'));
    return value && typeof value === 'object' && !Array.isArray(value) ? value : null;
  } catch { return null; }
}

export function writeJobJson(file, value, operations = fs) {
  operations.mkdirSync(path.dirname(file), { recursive: true });
  const temp = `${file}.${process.pid}.tmp`;
  try {
    operations.writeFileSync(temp, `${JSON.stringify(value, null, 2)}\n`);
    operations.renameSync(temp, file);
  } finally { operations.rmSync(temp, { force: true }); }
}
