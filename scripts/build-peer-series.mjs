// Caller: scripts build-peer.mjs (run-series), build-peer-series.test.mjs; skill build
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { parseCheckRun } from './build-peer-lib.mjs';

const SCRATCH_ENV = {
  GIT_AUTHOR_NAME: 'codeterm-peer-series', GIT_AUTHOR_EMAIL: 'peer-series@codeterm.invalid',
  GIT_COMMITTER_NAME: 'codeterm-peer-series', GIT_COMMITTER_EMAIL: 'peer-series@codeterm.invalid',
};

export function probeText(text, from, to) {
  if (from === null) return `${text}${text.endsWith('\n') || !text ? '' : '\n'}${to}\n`;
  const lines = text.split('\n');
  const at = lines.indexOf(from);
  if (at < 0) throw new Error('probe line missing');
  if (to === null) lines.splice(at, 1);
  else lines[at] = to;
  return lines.join('\n');
}

// A scratch commit is built with plumbing on a private index, so the checkout running the series never moves.
function scratchCommitter(git) {
  const index = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'peer-series-')), 'index');
  return (parent, file, mode, text, message) => {
    const env = { ...SCRATCH_ENV, GIT_INDEX_FILE: index };
    const blob = git(['hash-object', '-w', '--stdin'], { input: text });
    git(['read-tree', parent], { env });
    git(['update-index', '--cacheinfo', `${mode},${blob},${file}`], { env });
    const tree = git(['write-tree'], { env });
    return git(['commit-tree', tree, '-p', parent, '-m', message], { env });
  };
}

// Seed, warm, floor, edit, revert for each area of each base, strictly in order; the first bad run stops the series.
export async function runSeries({ spec, git, runCheck, results = [], onRun = () => {} }) {
  const expected = spec.bases.reduce((sum, base) => sum + base.areas.length, 0);
  const commit = scratchCommitter(git);
  let stopped = null;
  const modeOf = (sha, file) => git(['ls-tree', sha, '--', file]).split(/\s/)[0] || null;
  for (const base of spec.bases) {
    let start = base.sha;
    for (const patch of spec.patches ?? []) {
      const mode = modeOf(start, patch.file);
      const text = mode ? git(['cat-file', 'blob', `${start}:${patch.file}`], { raw: true }) : '';
      if (!mode || text.split(patch.find).length !== 2) { stopped = `patch_not_unique:${patch.file}@${base.label}`; break; }
      start = commit(start, patch.file, mode, text.replace(patch.find, () => patch.replace), `scratch prerequisite ${patch.file}`);
    }
    if (stopped) break;
    for (const area of base.areas) {
      const result = { base_label: base.label, base: base.sha, patched_base: start, area: area.name, file: area.file, seed: null, edit: null, revert: null, runs: {} };
      results.push(result);
      const mode = modeOf(start, area.file);
      if (!mode) { stopped = `missing_file:${area.file}@${base.label}`; break; }
      const original = git(['cat-file', 'blob', `${start}:${area.file}`], { raw: true });
      const seedText = probeText(original, null, spec.probe0);
      const editText = probeText(seedText, spec.probe0, spec.probe1);
      result.seed = commit(start, area.file, mode, seedText, `scratch seed ${area.name}`);
      result.edit = commit(result.seed, area.file, mode, editText, `scratch edit ${area.name}`);
      result.trees = Object.fromEntries([['base', base.sha], ['patched_base', start], ['seed', result.seed], ['edit', result.edit]]
        .map(([key, sha]) => [key, git(['rev-parse', `${sha}^{tree}`])]));
      for (const [role, sha] of [['warm', result.seed], ['floor', result.seed], ['edit', result.edit]]) {
        const run = await runCheck(sha);
        const logPath = parseCheckRun(run.output).fullLog;
        const parsed = parseCheckRun(run.output, logPath && fs.existsSync(logPath) ? fs.readFileSync(logPath, 'utf8') : '');
        result.runs[role] = { sha, code: run.code, timedOut: run.timedOut, ...parsed };
        onRun(results);
        if (run.timedOut) stopped = `deadline:${area.name}/${role}`;
        else if (parsed.skipped) stopped = `skipped_no_rust_changes:${area.name}/${role}`;
        else if (run.code !== 0 || !parsed.line) stopped = `${run.code === 75 ? 'busy' : 'failed'}:${area.name}/${role}:exit=${run.code}`;
        if (stopped) break;
      }
      if (stopped) break;
      result.revert = commit(result.edit, area.file, mode, probeText(editText, spec.probe1, null), `scratch revert ${area.name}`);
      if (git(['grep', '-n', spec.marker, result.revert, '--', '.'], { allowFail: true })) { stopped = `probe_left:${area.name}`; break; }
      if (git(['rev-parse', `${result.revert}^{tree}`]) !== git(['rev-parse', `${start}^{tree}`])) { stopped = `revert_mismatch:${area.name}`; break; }
    }
    if (stopped) break;
  }
  return { results, expected, stopped };
}

export function otherGatewayJobs(statusJson, ownJobs, since) {
  const recent = statusJson?.recent ?? [];
  return recent
    .filter((job) => job.started_at && Date.parse(job.started_at) >= since && !ownJobs.has(job.job_id))
    .map((job) => `${job.job_id}:${String(job.job ?? job.command ?? '?').split(' ')[0]}`);
}

export function gatewayDirOf(results) {
  for (const result of results) {
    for (const run of Object.values(result.runs)) {
      if (run.fullLog) return path.dirname(path.dirname(run.fullLog));
    }
  }
  return null;
}
