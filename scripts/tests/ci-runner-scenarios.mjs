import { evaluateCondition } from './workflow-condition.mjs';

export function runnerKinds(job) {
  const matrix = job.strategy?.matrix || {};
  let rows = [{}];
  for (const [key,values] of Object.entries(matrix)) {
    if (key === 'include' || key === 'exclude' || !Array.isArray(values)) continue;
    rows = rows.flatMap(row => values.map(value => ({...row,[key]:value})));
  }
  rows.push(...(matrix.include || []));
  const hostedVars = { CI_LINUX_LIGHT_RUNNER:'ubuntu-24.04',CI_LINUX_RUST_COMPILE_RUNNER:'ubuntu-24.04',
    CI_LINUX_RUST_FULL_RUNNER:'ubuntu-24.04',CI_MACOS_RUST_RUNNER:'macos-15',CI_MACOS_EMBEDDINGS_RUNNER:'macos-15' };
  const kinds = new Set();
  for (const vars of [{},hostedVars]) for (const row of rows) {
    const expression = job['runs-on'];
    const labels = typeof expression==='string' && expression.startsWith('$')
      ? evaluateCondition(expression,{vars,matrix:row,inputs:{}}) : expression;
    kinds.add((Array.isArray(labels) ? labels : [labels]).includes('self-hosted') ? 'self-hosted' : 'github-hosted');
  }
  return [...kinds];
}
