import { evaluateCondition } from './workflow-condition.mjs';

export function runnerScenarios(job) {
  const matrix = job.strategy?.matrix || {};
  let rows = [{}];
  for (const [key,values] of Object.entries(matrix)) {
    if (key === 'include' || key === 'exclude' || !Array.isArray(values)) continue;
    rows = rows.flatMap(row => values.map(value => ({...row,[key]:value})));
  }
  if (matrix.include && rows.length===1 && !Object.keys(rows[0]).length) rows = matrix.include;
  else rows.push(...(matrix.include || []));
  const hostedVars = { CI_LINUX_LIGHT_RUNNER:'ubuntu-24.04',CI_LINUX_RUST_COMPILE_RUNNER:'ubuntu-24.04',
    CI_LINUX_RUST_FULL_RUNNER:'ubuntu-24.04',CI_MACOS_RUST_RUNNER:'macos-15',CI_MACOS_EMBEDDINGS_RUNNER:'macos-15' };
  const scenarios = new Map();
  for (const vars of [{},hostedVars]) for (const row of rows) {
    const expression = job['runs-on'];
    const labels = typeof expression==='string' && expression.startsWith('$')
      ? evaluateCondition(expression,{vars,matrix:row,inputs:{}}) : expression;
    const kind = (Array.isArray(labels) ? labels : [labels]).includes('self-hosted') ? 'self-hosted' : 'github-hosted';
    scenarios.set(kind+JSON.stringify(row),{kind,matrix:row,labels});
  }
  return [...scenarios.values()];
}

export const runnerKinds = job => [...new Set(runnerScenarios(job).map(row=>row.kind))];
