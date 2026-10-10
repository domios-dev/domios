import { writeFileSync } from 'node:fs';
import path from 'node:path';
import { call, save } from './runtime.mjs';
import { verdict } from './model.mjs';

export function diagnosticResults(results) {
  return results.map(result => {
    const { output, ...diagnostic } = result;
    if (verdict([result]) !== 'PASSED' && output) {
      diagnostic.output = output.replace(/#< CLIXML\r?\n/g, '').replace(/<Obj\b[^>]*\bS="progress"[^>]*>[\s\S]*?<\/Obj>/g, '').replace(/<Objs\b[^>]*>\s*<\/Objs>/g, '').trim();
    }
    return diagnostic;
  });
}

export function deliver(binary, binding, root, content, dependencies = {}) {
  const invoke = dependencies.call || call;
  const write = dependencies.write || writeFileSync;
  const persist = dependencies.save || save;
  write(path.join(root, 'verdict.txt'), content, 'utf8');
  const pad = invoke(binary, ['pad', 'path'], binding.lane);
  const name = `lane-verdict-${binding.worker}.txt`;
  write(path.join(pad, name), content, 'utf8');
  const routing = [];
  for (const tab of [binding.worker, binding.orchestrator]) {
    try { routing.push({ tab, result: invoke(binary, ['send', '--tab', tab, '--from-pad', name, '--require-agent'], binding.lane, 30000) }); }
    catch (error) { routing.push({ tab, unconfirmed: String(error) }); }
    persist(path.join(root, 'routing.json'), routing);
  }
  return routing;
}
