// Caller: build.sh cache-stats
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { readJobJson } from './build-gateway-record.mjs';

export function summarizeCacheStats(records) {
  const grouped = new Map();
  for (const record of records) {
    const kind = record.job ?? record.command?.trim().split(/\s+/, 1)[0] ?? 'unknown';
    const row = grouped.get(kind) ?? { kind, jobs: 0, measured_jobs: 0, unmeasured_jobs: 0, requests: 0, hits: 0, misses: 0, non_cacheable: 0, reasons: {} };
    row.jobs++;
    const delta = record.sccache?.delta;
    if (!delta) {
      row.unmeasured_jobs++;
    } else {
      row.measured_jobs++;
      for (const key of ['requests', 'hits', 'misses', 'non_cacheable']) row[key] += Number(delta[key]) || 0;
      for (const [reason, count] of Object.entries(delta.not_cached ?? {})) row.reasons[reason] = (row.reasons[reason] ?? 0) + count;
    }
    grouped.set(kind, row);
  }
  return [...grouped.values()].sort((a, b) => a.kind.localeCompare(b.kind)).map(row => ({
    ...row,
    hit_rate: row.hits + row.misses ? row.hits / (row.hits + row.misses) : null,
  }));
}

export function formatCacheStats(rows) {
  const lines = [
    'Gateway job deltas by kind (retained per-job measurements; cache hit rate = hits / (hits + misses)):',
    'KIND JOBS MEASURED UNMEASURED REQUESTS HITS MISSES HIT_RATE NON_CACHEABLE',
  ];
  for (const row of rows) {
    const rate = row.hit_rate === null ? '-' : `${(row.hit_rate * 100).toFixed(1)}%`;
    lines.push(`${row.kind} ${row.jobs} ${row.measured_jobs} ${row.unmeasured_jobs} ${row.requests} ${row.hits} ${row.misses} ${rate} ${row.non_cacheable}`);
    const reasons = Object.entries(row.reasons).sort(([a], [b]) => a.localeCompare(b));
    if (reasons.length) lines.push(`  not cacheable: ${reasons.map(([reason, count]) => `${reason}=${count}`).join(', ')}`);
  }
  if (!rows.length) lines.push('No gateway job records found.');
  return lines.join('\n');
}

function readRecords(directory) {
  const jobs = path.join(directory, 'jobs');
  if (!fs.existsSync(jobs)) return [];
  return fs.readdirSync(jobs).filter(name => name.endsWith('.json')).flatMap(name => {
    const record = readJobJson(path.join(jobs, name));
    return record ? [record] : [];
  });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const index = process.argv.indexOf('--dir');
  if (index < 0 || !process.argv[index + 1]) {
    console.error('usage: node scripts/cache-stats.mjs --dir <gateway-dir>');
    process.exitCode = 2;
  } else {
    process.stdout.write(`${formatCacheStats(summarizeCacheStats(readRecords(process.argv[index + 1])))}\n`);
  }
}
