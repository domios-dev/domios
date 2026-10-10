import path from 'node:path';
import { options, storage, save } from './runtime.mjs';
const binding = options(process.argv.slice(2, 10));
const extras = process.argv.slice(10);
if (extras.length !== 4 || extras[0] !== '--session' || extras[2] !== '--generation' || !/^[a-zA-Z0-9_-]+$/.test(extras[1]) || !/^[a-zA-Z0-9_-]+$/.test(extras[3])) throw new Error('exact --session and --generation required');
const file = path.join(storage(binding), 'binding.json');
save(file, { worker: binding.worker, lane: binding.lane, session: extras[1], generation: extras[3] });
console.log(file);
