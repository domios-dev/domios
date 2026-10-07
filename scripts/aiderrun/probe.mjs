import { options, storage } from './runtime.mjs';
import { observed } from './turn.mjs';
try {
  const binding = options(process.argv.slice(2));
  const turn = observed(binding, storage(binding));
  console.log(turn ? `completed ${turn.key} ${turn.outcome}` : 'waiting');
} catch (error) { console.log(`failed ${error.message}`); process.exitCode = 1; }
