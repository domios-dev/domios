// Caller: build.sh build-history; scripts/build-history.test.mjs
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { readJobJson } from './build-gateway-record.mjs';

export function median(values) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 ? sorted[middle] : (sorted[middle - 1] + sorted[middle]) / 2;
}

export function summarizeHistory(records, { last = 50, verb, now, canonical } = {}) {
  if (!Number.isSafeInteger(last) || last < 1) throw new Error('--last must be a positive integer');
  if (!Number.isFinite(now)) throw new Error('history requires an explicit clock');
  const normalize = value => {
    const normalized = String(value ?? '').replaceAll('\\', '/').replace(/\/$/, '');
    return /^[a-z]:\//i.test(normalized) ? normalized.toLowerCase() : normalized;
  };
  const jobs = records.filter(record => !verb || (record.job ?? record.command?.split(' ')[0]) === verb)
    .sort((a, b) => String(b.started_at ?? b.queued_at ?? '').localeCompare(String(a.started_at ?? a.queued_at ?? '')) || String(b.job_id).localeCompare(String(a.job_id)))
    .slice(0, last).map(record => {
      const start = Date.parse(record.started_at);
      const end = record.state === 'running' ? now : Date.parse(record.ended_at);
      const duration_ms = Number.isFinite(start) && Number.isFinite(end) && end >= start ? end - start : null;
      const worktree = normalize(record.worktree);
      const worktree_label = canonical && worktree === normalize(canonical) ? 'canonical' : String(record.worktree ?? '').replaceAll('\\', '/').replace(/\/$/, '').split('/').at(-1) || 'unknown';
      return { ...record, verb: record.job ?? record.command?.split(' ')[0] ?? 'unknown', duration_ms, worktree_label };
    });
  const verbs = [...new Set(jobs.map(job => job.verb))].sort().map(verb => {
    const durations = jobs.filter(job => job.verb === verb && job.state === 'completed' && job.exit_code === 0 && job.duration_ms !== null).map(job => job.duration_ms);
    const sorted = [...durations].sort((a, b) => a - b);
    const recent = durations.slice(0, 10);
    const previous = durations.slice(10, 20);
    const last_median_ms = median(recent);
    const previous_median_ms = median(previous);
    const complete = recent.length === 10 && previous.length === 10;
    return {
      verb, n: durations.length, median_ms: median(durations),
      p90_ms: sorted.length ? sorted[Math.ceil(sorted.length * 0.9) - 1] : null,
      max_ms: sorted.at(-1) ?? null,
      trend: { last_n: recent.length, previous_n: previous.length, last_median_ms, previous_median_ms,
        delta_ms: complete ? last_median_ms - previous_median_ms : null,
        percent: complete && previous_median_ms > 0 ? (last_median_ms / previous_median_ms - 1) * 100 : null },
    };
  });
  return { schema: 1, generated_at: new Date(now).toISOString(), last, verb: verb ?? null, jobs, verbs };
}

function duration(value) { return value === null ? '-' : `${(value / 1000).toFixed(1)}s`; }
function table(rows) {
  const safe = rows.map(row => row.map(value => String(value).replace(/[\r\n\t\x1b]/g, ' ')));
  const widths = safe[0].map((_, index) => Math.max(...safe.map(row => row[index].length)));
  return safe.map(row => row.map((value, index) => value.padEnd(widths[index])).join('  ').trimEnd()).join('\n');
}

export function formatHistory(history) {
  const rows = [['TIME (UTC)', 'VERB', 'DURATION', 'EXIT', 'WORKTREE', 'SHA', 'CRATES/FILES', 'CACHE HIT']];
  for (const job of history.jobs) {
    const cache = job.sccache?.delta;
    rows.push([job.started_at ?? job.queued_at ?? '-', job.verb,
      `${duration(job.duration_ms)}${job.state === 'running' ? ' elapsed' : ''}`,
      job.state === 'running' || job.state === 'queued' ? job.state : job.exit_code ?? job.state,
      job.worktree_label, (job.source_sha ?? '-').slice(0, 12),
      job.crates?.join(',') || (job.touched_files_count == null ? '-' : `${job.touched_files_count} files`),
      cache?.hit_rate == null ? '-' : `${(cache.hit_rate * 100).toFixed(1)}% (${cache.hits}/${cache.hits + cache.misses})`]);
  }
  const stats = [['VERB (successful)', 'N', 'MEDIAN', 'P90', 'MAX', 'TREND (last 10 / previous 10)']];
  for (const item of history.verbs) {
    const trend = item.trend;
    stats.push([item.verb, item.n, duration(item.median_ms), duration(item.p90_ms), duration(item.max_ms),
      trend.delta_ms === null ? `insufficient samples (${trend.last_n}/10, ${trend.previous_n}/10)` :
        `${duration(trend.last_median_ms)} / ${duration(trend.previous_median_ms)} (${trend.percent === null ? 'zero baseline' : `${trend.percent >= 0 ? '+' : ''}${trend.percent.toFixed(1)}%`})`]);
  }
  const details = [];
  for (const job of history.jobs) {
    const rebuilds = job.diagnostics?.rebuilt_units ?? [];
    const noncacheable = job.diagnostics?.non_cacheable_units ?? [];
    if (!rebuilds.length && !noncacheable.length && !job.sccache?.delta?.non_cacheable) continue;
    details.push(`\n${job.job_id} ${job.verb}:`);
    const ranked = [...rebuilds].sort((a, b) => b.count - a.count || a.unit.localeCompare(b.unit));
    for (const unit of ranked.slice(0, 10)) details.push(`  rebuilt ${unit.unit} (${unit.count}): ${unit.reasons.join('; ')}`);
    for (const unit of noncacheable.slice(0, 10)) details.push(`  non-cacheable ${unit.unit}: ${unit.reason}`);
    if (job.sccache?.delta?.non_cacheable) {
      details.push(`  non-cacheable requests: ${job.sccache.delta.non_cacheable}; reasons: ${JSON.stringify(job.sccache.delta.not_cached)}`);
      if (!noncacheable.length) details.push('  per-unit sccache evidence unavailable (server debug log required)');
    }
  }
  return `${table(rows)}\n\n${table(stats)}\n${details.join('\n')}\n`;
}

export function parseHistoryArgs(args) {
  const options = { last: 50, json: false };
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (arg === '--json') options.json = true;
    else if (['--last', '--verb', '--dir', '--canonical'].includes(arg)) {
      const value = args[++i];
      if (!value || value.startsWith('--')) throw new Error(`${arg} requires a value`);
      options[arg.slice(2)] = arg === '--last' && /^\d+$/.test(value) ? Number(value) : value;
    } else throw new Error(`unknown build-history argument: ${arg}`);
  }
  return options;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const options = parseHistoryArgs(process.argv.slice(2));
    const directory = path.join(options.dir, 'jobs');
    const records = fs.existsSync(directory) ? fs.readdirSync(directory).filter(name => name.endsWith('.json')).flatMap(name => {
      const record = readJobJson(path.join(directory, name));
      if (record) return [record];
      console.error(`build-history: skipped unreadable record ${name}`);
      return [];
    }) : [];
    const history = summarizeHistory(records, { ...options, now: Date.now() });
    process.stdout.write(options.json ? `${JSON.stringify(history, null, 2)}\n` : formatHistory(history));
  } catch (error) { console.error(`build-history: ${error.message}`); process.exitCode = 2; }
}
